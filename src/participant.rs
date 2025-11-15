//!
//! participant.rs
//! Implementation of 2PC participant
//!
extern crate ipc_channel;
extern crate log;
extern crate rand;
extern crate stderrlog;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use std::thread;

use participant::rand::prelude::*;
use participant::ipc_channel::ipc::IpcReceiver as Receiver;
use participant::ipc_channel::ipc::TryRecvError;
use participant::ipc_channel::ipc::IpcSender as Sender;

use message::MessageType;
use message::ProtocolMessage;
use oplog;

///
/// ParticipantState
/// enum for Participant 2PC state machine
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticipantState {
    Quiescent,
    ReceivedP1,
    VotedAbort,
    VotedCommit,
    AwaitingGlobalDecision,
}

///
/// Participant
/// Structure for maintaining per-participant state and communication/synchronization objects to/from coordinator
///
pub struct Participant {
    id_str: String,
    state: ParticipantState,
    log: oplog::OpLog,
    running: Arc<AtomicBool>,
    send_success_prob: f64,
    operation_success_prob: f64,
    tx: Sender<ProtocolMessage>,
    rx: Receiver<ProtocolMessage>,
    successful_ops: u64,
    failed_ops: u64,
    unknown_ops: u64,
}

///
/// Participant
/// Implementation of participant for the 2PC protocol
/// Required:
/// 1. new -- Constructor
/// 2. pub fn report_status -- Reports number of committed/aborted/unknown for each participant
/// 3. pub fn protocol() -- Implements participant side protocol for 2PC
///
impl Participant {

    ///
    /// new()
    ///
    /// Return a new participant, ready to run the 2PC protocol with the coordinator.
    ///
    /// HINT: You may want to pass some channels or other communication
    ///       objects that enable coordinator->participant and participant->coordinator
    ///       messaging to this constructor.
    /// HINT: You may want to pass some global flags that indicate whether
    ///       the protocol is still running to this constructor. There are other
    ///       ways to communicate this, of course.
    ///
    pub fn new(
        id_str: String,
        log_path: String,
        r: Arc<AtomicBool>,
        send_success_prob: f64,
        operation_success_prob: f64,
        tx: Sender<ProtocolMessage>,
        rx: Receiver<ProtocolMessage>) -> Participant {

        Participant {
            id_str: id_str,
            state: ParticipantState::Quiescent,
            log: oplog::OpLog::new(log_path),
            running: r,
            send_success_prob: send_success_prob,
            operation_success_prob: operation_success_prob,
            tx: tx,
            rx: rx,
            successful_ops: 0,
            failed_ops: 0,
            unknown_ops: 0,
        }
    }

    ///
    /// send()
    /// Send a protocol message to the coordinator. This can fail depending on
    /// the success probability. For testing purposes, make sure to not specify
    /// the -S flag so the default value of 1 is used for failproof sending.
    ///
    /// HINT: You will need to implement the actual sending
    ///
    pub fn send(&mut self, pm: ProtocolMessage) {
        let x: f64 = random();
        if x <= self.send_success_prob {
            self.tx.send(pm.clone()).unwrap_or(());
            trace!("{}::Sent message successfully", self.id_str);
        } else {
            trace!("{}::Failed to send message", self.id_str);
        }
    }

    ///
    /// perform_operation
    /// Perform the operation specified in the 2PC proposal,
    /// with some probability of success/failure determined by the
    /// command-line option success_probability.
    ///
    /// HINT: The code provided here is not complete--it provides some
    ///       tracing infrastructure and the probability logic.
    ///       Your implementation need not preserve the method signature
    ///       (it's ok to add parameters or return something other than
    ///       bool if it's more convenient for your design).
    ///
    pub fn perform_operation(&mut self, request_option: &Option<ProtocolMessage>) -> bool {

        trace!("{}::Performing operation", self.id_str.clone());
        let x: f64 = random();
        if x <= self.operation_success_prob {
            trace!("{}::Operation successful", self.id_str);
            true
        } else {
            trace!("{}::Operation failed", self.id_str);
            false
        }
    }

    ///
    /// report_status()
    /// Report the abort/commit/unknown status (aggregate) of all transaction
    /// requests made by this coordinator before exiting.
    ///
    pub fn report_status(&mut self) {
        println!("{}:\tC:{}\tA:{}\tU:{}", self.id_str, self.successful_ops, self.failed_ops, self.unknown_ops);
    }

    ///
    /// wait_for_exit_signal(&mut self)
    /// Wait until the running flag is set by the CTRL-C handler
    ///
    pub fn wait_for_exit_signal(&mut self) {
        trace!("{}::Waiting for exit signal", self.id_str.clone());

        loop {
            match self.rx.try_recv() {
                Ok(msg) => {
                    if msg.mtype == MessageType::CoordinatorExit {
                        trace!("{}::Received exit signal", self.id_str);
                        break;
                    }
                },
                Err(TryRecvError::Empty) => {
                    if !self.running.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                },
                Err(TryRecvError::IpcError(_)) => {
                    break;
                }
            }
        }

        trace!("{}::Exiting", self.id_str.clone());
    }

    ///
    /// protocol()
    /// Implements the participant side of the 2PC protocol
    /// HINT: If the simulation ends early, don't keep handling requests!
    /// HINT: Wait for some kind of exit signal before returning from the protocol!
    ///
    pub fn protocol(&mut self) {
        trace!("{}::Beginning protocol", self.id_str.clone());

        loop {
            if !self.running.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }

            // Wait for proposal from coordinator
            match self.rx.try_recv() {
                Ok(msg) => {
                    if msg.mtype == MessageType::CoordinatorExit {
                        trace!("{}::Received exit signal in protocol", self.id_str);
                        break;
                    } else if msg.mtype == MessageType::CoordinatorPropose {
                        trace!("{}::Received proposal for txid: {}", self.id_str, msg.txid);
                        self.state = ParticipantState::ReceivedP1;

                        // Perform operation to decide vote
                        let success = self.perform_operation(&Some(msg.clone()));

                        let vote_msg = if success {
                            self.state = ParticipantState::VotedCommit;
                            info!("{}::Voting COMMIT for txid: {}", self.id_str, msg.txid);
                            // Log the local vote commit
                            self.log.append(
                                MessageType::ParticipantVoteCommit,
                                msg.txid.clone(),
                                self.id_str.clone(),
                                msg.opid,
                            );
                            ProtocolMessage::generate(
                                MessageType::ParticipantVoteCommit,
                                msg.txid.clone(),
                                self.id_str.clone(),
                                msg.opid,
                            )
                        } else {
                            self.state = ParticipantState::VotedAbort;
                            info!("{}::Voting ABORT for txid: {}", self.id_str, msg.txid);
                            ProtocolMessage::generate(
                                MessageType::ParticipantVoteAbort,
                                msg.txid.clone(),
                                self.id_str.clone(),
                                msg.opid,
                            )
                        };

                        // Send vote to coordinator
                        self.send(vote_msg);
                        self.state = ParticipantState::AwaitingGlobalDecision;

                        // Wait for global decision from coordinator
                        let timeout = Duration::from_millis(2000);
                        let start_time = std::time::Instant::now();
                        let mut decision_received = false;

                        while start_time.elapsed() < timeout {
                            if !self.running.load(std::sync::atomic::Ordering::SeqCst) {
                                break;
                            }

                            match self.rx.try_recv() {
                                Ok(decision_msg) => {
                                    if decision_msg.txid == msg.txid {
                                        if decision_msg.mtype == MessageType::CoordinatorCommit {
                                            info!("{}::Received COMMIT decision for txid: {}", self.id_str, msg.txid);
                                            self.successful_ops += 1;
                                            // Log the global commit decision
                                            self.log.append(
                                                MessageType::CoordinatorCommit,
                                                msg.txid.clone(),
                                                self.id_str.clone(),
                                                msg.opid,
                                            );
                                            decision_received = true;
                                            break;
                                        } else if decision_msg.mtype == MessageType::CoordinatorAbort {
                                            info!("{}::Received ABORT decision for txid: {}", self.id_str, msg.txid);
                                            self.failed_ops += 1;
                                            // Log the global abort decision
                                            self.log.append(
                                                MessageType::CoordinatorAbort,
                                                msg.txid.clone(),
                                                self.id_str.clone(),
                                                msg.opid,
                                            );
                                            decision_received = true;
                                            break;
                                        } else if decision_msg.mtype == MessageType::CoordinatorExit {
                                            trace!("{}::Received exit signal while waiting for decision", self.id_str);
                                            self.unknown_ops += 1;
                                            decision_received = true;
                                            break;
                                        }
                                    }
                                },
                                Err(TryRecvError::Empty) => {
                                    thread::sleep(Duration::from_millis(10));
                                },
                                Err(TryRecvError::IpcError(_)) => {
                                    break;
                                }
                            }
                        }

                        if !decision_received {
                            trace!("{}::Timeout waiting for decision on txid: {}", self.id_str, msg.txid);
                            self.unknown_ops += 1;
                        }

                        self.state = ParticipantState::Quiescent;
                    }
                },
                Err(TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(10));
                },
                Err(TryRecvError::IpcError(_)) => {
                    break;
                }
            }
        }

        self.wait_for_exit_signal();
        self.report_status();
    }
}
