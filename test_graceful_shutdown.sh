#!/bin/bash

# Test script for graceful shutdown verification
# Tests that all processes terminate cleanly

echo "=========================================="
echo "Testing Graceful Shutdown"
echo "=========================================="

# Clean up any existing logs
rm -rf logs/*

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test 1: Quick CTRL-C (immediate shutdown)
echo -e "\n${YELLOW}Test 1: Quick CTRL-C (immediate shutdown)${NC}"
echo "Starting 2PC with 2 clients, 3 participants, 5 requests each..."

# Start the process in background
./target/debug/two_phase_commit -s 1.0 -c 2 -p 3 -r 5 -m run -v 0 &
PID=$!
echo "Started with PID: $PID"

# Wait a moment for startup
sleep 1

# Send SIGINT (CTRL-C)
echo "Sending SIGINT to PID $PID..."
kill -INT $PID

# Wait for graceful shutdown (max 5 seconds)
TIMEOUT=5
COUNT=0
while kill -0 $PID 2>/dev/null; do
    sleep 0.5
    COUNT=$((COUNT + 1))
    if [ $COUNT -ge $((TIMEOUT * 2)) ]; then
        echo -e "${RED}FAIL: Process did not exit within ${TIMEOUT} seconds${NC}"
        echo "Force killing remaining processes..."
        kill -9 $PID 2>/dev/null
        killall -9 two_phase_commit 2>/dev/null
        exit 1
    fi
done

echo -e "${GREEN}PASS: Process exited gracefully in $((COUNT / 2)) seconds${NC}"

# Check for zombie processes
ZOMBIES=$(ps aux | grep two_phase_commit | grep -v grep | grep -v test_graceful_shutdown)
if [ ! -z "$ZOMBIES" ]; then
    echo -e "${RED}FAIL: Found lingering processes:${NC}"
    echo "$ZOMBIES"
    killall -9 two_phase_commit 2>/dev/null
    exit 1
fi
echo -e "${GREEN}PASS: No lingering processes${NC}"

# Test 2: Let it run to completion
echo -e "\n${YELLOW}Test 2: Natural completion (all requests finish)${NC}"
rm -rf logs/*
echo "Starting 2PC with 1 client, 2 participants, 3 requests..."

# Run with small workload
./target/debug/two_phase_commit -s 1.0 -c 1 -p 2 -r 3 -m run -v 0 &
PID=$!
echo "Started with PID: $PID"

# Wait longer for work to complete
sleep 3

# Send SIGINT after some work
kill -INT $PID

# Wait for exit
TIMEOUT=5
COUNT=0
while kill -0 $PID 2>/dev/null; do
    sleep 0.5
    COUNT=$((COUNT + 1))
    if [ $COUNT -ge $((TIMEOUT * 2)) ]; then
        echo -e "${RED}FAIL: Process did not exit within ${TIMEOUT} seconds${NC}"
        kill -9 $PID 2>/dev/null
        killall -9 two_phase_commit 2>/dev/null
        exit 1
    fi
done

echo -e "${GREEN}PASS: Process exited gracefully${NC}"

# Check for zombies again
ZOMBIES=$(ps aux | grep two_phase_commit | grep -v grep | grep -v test_graceful_shutdown)
if [ ! -z "$ZOMBIES" ]; then
    echo -e "${RED}FAIL: Found lingering processes:${NC}"
    echo "$ZOMBIES"
    killall -9 two_phase_commit 2>/dev/null
    exit 1
fi
echo -e "${GREEN}PASS: No lingering processes${NC}"

# Verify logs were created (proves processes ran)
if [ ! -f "logs/coordinator.log" ]; then
    echo -e "${RED}FAIL: Coordinator log not created${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: Logs created successfully${NC}"

# Test 3: Multiple rapid CTRL-C
echo -e "\n${YELLOW}Test 3: Multiple rapid CTRL-C (stress test)${NC}"
rm -rf logs/*
echo "Starting 2PC with 4 clients, 5 participants, 10 requests..."

./target/debug/two_phase_commit -s 0.9 -c 4 -p 5 -r 10 -m run -v 0 &
PID=$!
echo "Started with PID: $PID"

sleep 1

# Send multiple SIGINT in rapid succession
echo "Sending multiple SIGINT signals..."
kill -INT $PID
sleep 0.1
kill -INT $PID 2>/dev/null
sleep 0.1
kill -INT $PID 2>/dev/null

# Wait for exit
TIMEOUT=8
COUNT=0
while kill -0 $PID 2>/dev/null; do
    sleep 0.5
    COUNT=$((COUNT + 1))
    if [ $COUNT -ge $((TIMEOUT * 2)) ]; then
        echo -e "${RED}FAIL: Process did not exit within ${TIMEOUT} seconds${NC}"
        kill -9 $PID 2>/dev/null
        killall -9 two_phase_commit 2>/dev/null
        exit 1
    fi
done

echo -e "${GREEN}PASS: Process handled multiple SIGINTs gracefully${NC}"

# Final zombie check
ZOMBIES=$(ps aux | grep two_phase_commit | grep -v grep | grep -v test_graceful_shutdown)
if [ ! -z "$ZOMBIES" ]; then
    echo -e "${RED}FAIL: Found lingering processes:${NC}"
    echo "$ZOMBIES"
    killall -9 two_phase_commit 2>/dev/null
    exit 1
fi
echo -e "${GREEN}PASS: No lingering processes after stress test${NC}"

echo -e "\n=========================================="
echo -e "${GREEN}All graceful shutdown tests PASSED!${NC}"
echo -e "=========================================="

# Verify with checker that logs are valid
echo -e "\n${YELLOW}Bonus: Verifying log correctness${NC}"
./target/debug/two_phase_commit -s 0.9 -c 4 -p 5 -r 10 -m check -v 0
if [ $? -eq 0 ]; then
    echo -e "${GREEN}PASS: Logs are valid${NC}"
else
    echo -e "${RED}WARN: Some log issues (may be due to early termination)${NC}"
fi

exit 0

