#!/bin/bash

# Simple manual test for graceful shutdown
# Run this and then press CTRL-C to test

echo "=========================================="
echo "Manual Graceful Shutdown Test"
echo "=========================================="
echo ""
echo "Instructions:"
echo "1. The program will start with 4 clients, 10 participants"
echo "2. Press CTRL-C after a few seconds"
echo "3. All processes should exit within 1-2 seconds"
echo "4. You should see status output from all participants"
echo "5. Script will check for lingering processes"
echo ""
echo "Starting in 3 seconds..."
sleep 1
echo "2..."
sleep 1
echo "1..."
sleep 1
echo ""

# Clean logs
rm -rf logs/*

# Run the program
echo "Program starting... (Press CTRL-C to test graceful shutdown)"
echo ""
./target/debug/two_phase_commit -s .95 -c 4 -p 10 -r 10 -m run

# After CTRL-C, check for lingering processes
echo ""
echo "Checking for lingering processes..."
sleep 1

LINGERING=$(ps aux | grep two_phase_commit | grep -v grep | grep -v manual_shutdown)
if [ ! -z "$LINGERING" ]; then
    echo "⚠️  WARNING: Found lingering processes:"
    echo "$LINGERING"
    echo ""
    echo "Cleaning up..."
    killall -9 two_phase_commit 2>/dev/null
    echo "❌ FAIL: Processes did not exit gracefully"
    exit 1
else
    echo "✅ SUCCESS: All processes exited gracefully!"
    echo ""
    echo "Verify output above shows status from:"
    echo "  - coordinator"
    echo "  - All 10 participants"  
    echo "  - All 4 clients (may finish at different times)"
fi

echo ""
echo "=========================================="

