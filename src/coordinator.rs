//!
//! coordinator.rs
//! Implementation of 2PC coordinator
//!
extern crate log;
extern crate stderrlog;
extern crate rand;
extern crate ipc_channel;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use coordinator::ipc_channel::ipc::IpcSender as Sender;
use coordinator::ipc_channel::ipc::IpcReceiver as Receiver;
use coordinator::ipc_channel::ipc::TryRecvError;

use message::MessageType;
use message::ProtocolMessage;
use oplog;

/// CoordinatorState
/// States for 2PC state machine
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinatorState {
    Quiescent,
    ReceivedRequest,
    ProposalSent,
    ReceivedVotesAbort,
    ReceivedVotesCommit,
    SentGlobalDecision
}

/// Coordinator
/// Struct maintaining state for coordinator
pub struct Coordinator {
    state: CoordinatorState,
    running: Arc<AtomicBool>,
    log: oplog::OpLog,
    participant_map: HashMap<String, (Sender<ProtocolMessage>, Receiver<ProtocolMessage>)>,
    client_map: HashMap<String, (Sender<ProtocolMessage>, Receiver<ProtocolMessage>)>,
    successful_ops: u64,
    failed_ops: u64,
    unknown_ops: u64,
}

///
/// Coordinator
/// Implementation of coordinator functionality
/// Required:
/// 1. new -- Constructor
/// 2. protocol -- Implementation of coordinator side of protocol
/// 3. report_status -- Report of aggregate commit/abort/unknown stats on exit.
/// 4. participant_join -- What to do when a participant joins
/// 5. client_join -- What to do when a client joins
///
impl Coordinator {

    ///
    /// new()
    /// Initialize a new coordinator
    ///
    /// <params>
    ///     log_path: directory for log files --> create a new log there.
    ///     r: atomic bool --> still running?
    ///
    pub fn new(
        log_path: String,
        r: &Arc<AtomicBool>) -> Coordinator {

        Coordinator {
            state: CoordinatorState::Quiescent,
            log: oplog::OpLog::new(log_path),
            running: r.clone(),
            participant_map: HashMap::new(),
            client_map: HashMap::new(),
            successful_ops: 0,
            failed_ops: 0,
            unknown_ops: 0,
        }
    }

    ///
    /// participant_join()
    /// Adds a new participant for the coordinator to keep track of
    ///
    /// HINT: Keep track of any channels involved!
    /// HINT: You may need to change the signature of this function
    ///
    pub fn participant_join(&mut self, name: &String, 
                           sender: Sender<ProtocolMessage>, 
                           receiver: Receiver<ProtocolMessage>) {
        assert!(self.state == CoordinatorState::Quiescent);

        self.participant_map.insert(name.clone(), (sender, receiver));
    }

    ///
    /// client_join()
    /// Adds a new client for the coordinator to keep track of
    ///
    /// HINT: Keep track of any channels involved!
    /// HINT: You may need to change the signature of this function
    ///
    pub fn client_join(&mut self, name: &String, 
                      sender: Sender<ProtocolMessage>, 
                      receiver: Receiver<ProtocolMessage>) {
        assert!(self.state == CoordinatorState::Quiescent);

        self.client_map.insert(name.clone(), (sender, receiver));
    }

    ///
    /// report_status()
    /// Report the abort/commit/unknown status (aggregate) of all transaction
    /// requests made by this coordinator before exiting.
    ///
    pub fn report_status(&mut self) {
        println!("coordinator:\tC:{}\tA:{}\tU:{}", self.successful_ops, self.failed_ops, self.unknown_ops);
    }

    ///
    /// protocol()
    /// Implements the coordinator side of the 2PC protocol
    /// HINT: If the simulation ends early, don't keep handling requests!
    /// HINT: Wait for some kind of exit signal before returning from the protocol!
    ///
    pub fn protocol(&mut self) {
        
        loop {
            if !self.running.load(Ordering::SeqCst) {
                break;
            }

            // Try to receive a request from any client
            let mut request_received = false;
            let mut request: Option<ProtocolMessage> = None;
            let mut client_name: Option<String> = None;

            for (name, (_tx, rx)) in self.client_map.iter() {
                match rx.try_recv() {
                    Ok(msg) => {
                        if msg.mtype == MessageType::ClientRequest {
                            trace!("Coordinator received request from {}", name);
                            request = Some(msg);
                            client_name = Some(name.clone());
                            request_received = true;
                            break;
                        }
                    },
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::IpcError(_)) => continue,
                }
            }

            if !request_received {
                thread::sleep(Duration::from_millis(1));
                continue;
            }

            let req = request.unwrap();
            let client_id = client_name.unwrap();
            
            // Phase 1: Send proposal to all participants
            self.state = CoordinatorState::ProposalSent;
            info!("Coordinator sending proposal for txid: {}", req.txid);
            
            let propose_msg = ProtocolMessage::generate(
                MessageType::CoordinatorPropose,
                req.txid.clone(),
                "coordinator".to_string(),
                req.opid,
            );

            // Send proposal to all participants
            for (_name, (tx, _rx)) in self.participant_map.iter() {
                tx.send(propose_msg.clone()).unwrap_or(());
            }

            // Phase 2: Collect votes from all participants
            let mut votes_commit = 0;
            let mut votes_abort = 0;
            let num_participants = self.participant_map.len();
            let timeout = Duration::from_millis(200);
            let start_time = std::time::Instant::now();

            while votes_commit + votes_abort < num_participants {
                if start_time.elapsed() > timeout {
                    trace!("Timeout waiting for votes on txid: {}", req.txid);
                    break;
                }

                if !self.running.load(Ordering::SeqCst) {
                    break;
                }

                // Early abort optimization: if any participant votes abort, we can decide immediately
                if votes_abort > 0 {
                    trace!("Early abort detected for txid: {}", req.txid);
                    break;
                }

                for (_name, (_tx, rx)) in self.participant_map.iter() {
                    match rx.try_recv() {
                        Ok(msg) => {
                            if msg.txid == req.txid {
                                if msg.mtype == MessageType::ParticipantVoteCommit {
                                    votes_commit += 1;
                                    trace!("Received commit vote for txid: {}", req.txid);
                                } else if msg.mtype == MessageType::ParticipantVoteAbort {
                                    votes_abort += 1;
                                    trace!("Received abort vote for txid: {}", req.txid);
                                }
                            }
                        },
                        Err(TryRecvError::Empty) => continue,
                        Err(TryRecvError::IpcError(_)) => continue,
                    }
                }

                thread::sleep(Duration::from_millis(1));
            }

            // Make decision
            let commit_decision = votes_commit == num_participants && votes_abort == 0;
            
            let (decision_msg_type, result_msg_type) = if commit_decision {
                self.successful_ops += 1;
                self.state = CoordinatorState::ReceivedVotesCommit;
                info!("Coordinator decided COMMIT for txid: {}", req.txid);
                (MessageType::CoordinatorCommit, MessageType::ClientResultCommit)
            } else {
                self.failed_ops += 1;
                self.state = CoordinatorState::ReceivedVotesAbort;
                info!("Coordinator decided ABORT for txid: {}", req.txid);
                (MessageType::CoordinatorAbort, MessageType::ClientResultAbort)
            };

            // Log the decision
            self.log.append(decision_msg_type, req.txid.clone(), "coordinator".to_string(), req.opid);

            // Send decision to all participants
            let decision_msg = ProtocolMessage::generate(
                decision_msg_type,
                req.txid.clone(),
                "coordinator".to_string(),
                req.opid,
            );

            for (_name, (tx, _rx)) in self.participant_map.iter() {
                tx.send(decision_msg.clone()).unwrap_or(());
            }

            // Send result to client
            let result_msg = ProtocolMessage::generate(
                result_msg_type,
                req.txid.clone(),
                "coordinator".to_string(),
                req.opid,
            );

            if let Some((tx, _rx)) = self.client_map.get(&client_id) {
                tx.send(result_msg).unwrap_or(());
            }

            self.state = CoordinatorState::SentGlobalDecision;
        }

        // Send exit messages to all clients and participants
        for (name, (tx, _rx)) in self.client_map.iter() {
            let exit_msg = ProtocolMessage::generate(
                MessageType::CoordinatorExit,
                "exit".to_string(),
                "coordinator".to_string(),
                0,
            );
            tx.send(exit_msg).unwrap_or(());
            trace!("Sent exit to client: {}", name);
        }

        for (name, (tx, _rx)) in self.participant_map.iter() {
            let exit_msg = ProtocolMessage::generate(
                MessageType::CoordinatorExit,
                "exit".to_string(),
                "coordinator".to_string(),
                0,
            );
            tx.send(exit_msg).unwrap_or(());
            trace!("Sent exit to participant: {}", name);
        }

        // Give children a moment to receive and process exit messages
        thread::sleep(Duration::from_millis(50));

        self.report_status();
    }
}
