#!/bin/bash

echo "=== Security Test Script ==="
echo "Testing agent user permissions..."
echo

# Test 1: Check if user can install packages
echo "1. Testing package installation permissions..."
if sudo -l | grep -q "apt-get\|apt\|dpkg"; then
    echo "✓ Agent user can install packages via apt"
else
    echo "✗ Agent user cannot install packages"
fi

# Test 2: Check if iptables commands are denied
echo
echo "2. Testing iptables access (should be denied)..."
if sudo -l | grep -q "!/.*iptables"; then
    echo "✓ iptables commands are properly denied"
else
    echo "✗ iptables commands are not properly restricted"
fi

# Test 3: Try to run iptables (should fail)
echo
echo "3. Attempting to run iptables command..."
if sudo iptables -L >/dev/null 2>&1; then
    echo "✗ SECURITY ISSUE: Agent user can run iptables!"
else
    echo "✓ iptables access properly blocked"
fi

# Test 4: Try to install a package (should work)
echo
echo "4. Testing package installation..."
if sudo apt-get update >/dev/null 2>&1; then
    echo "✓ Package manager access works"
else
    echo "✗ Package manager access failed"
fi

echo
echo "=== Security Test Complete ==="
echo
echo "Expected results:"
echo "✓ Agent can install packages"
echo "✓ Agent cannot modify iptables"
echo "✓ Transparent proxy remains secure" 