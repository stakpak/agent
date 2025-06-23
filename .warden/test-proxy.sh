#!/bin/bash

echo "=== Transparent Proxy Test Script ==="
echo

# Test 1: Check if redsocks is running
echo "1. Checking if redsocks is running..."
if pgrep -x redsocks > /dev/null; then
    echo "✓ Redsocks is running"
    echo "   PID: $(pgrep -x redsocks)"
else
    echo "✗ Redsocks is not running"
    exit 1
fi
echo

# Test 2: Check iptables rules
echo "2. Checking iptables rules..."
if iptables -t nat -L OUTPUT | grep -q REDSOCKS; then
    echo "✓ Iptables redirect rule is active"
else
    echo "✗ Iptables redirect rule not found"
fi

if iptables -t nat -L REDSOCKS | grep -q "REDIRECT.*12345"; then
    echo "✓ REDSOCKS chain has redirect to port 12345"
else
    echo "✗ REDSOCKS chain redirect rule not found"
fi
echo

# Test 3: Check if redsocks port is listening
echo "3. Checking if redsocks is listening on port 12345..."
if netstat -ln | grep ":12345" > /dev/null 2>&1; then
    echo "✓ Redsocks is listening on port 12345"
else
    echo "✗ Redsocks is not listening on port 12345"
fi
echo

# Test 4: Test external connectivity
echo "4. Testing external connectivity..."
echo "   Testing httpbin.org/ip (should show proxy IP if working)..."
if curl -s --connect-timeout 10 --max-time 30 https://httpbin.org/ip; then
    echo "✓ External HTTPS connectivity working"
else
    echo "✗ External HTTPS connectivity failed"
fi
echo

# Test 5: Test what IP is seen externally
echo "5. Testing external IP visibility..."
EXTERNAL_IP=$(curl -s --connect-timeout 10 --max-time 30 https://httpbin.org/ip | grep -o '"origin": "[^"]*"' | cut -d'"' -f4)
if [ -n "$EXTERNAL_IP" ]; then
    echo "   External services see this IP: $EXTERNAL_IP"
    echo "   (This should be your proxy server's IP if transparent proxy is working)"
else
    echo "✗ Could not determine external IP"
fi
echo

echo "=== Test Complete ==="
echo
echo "To verify transparent proxy is working:"
echo "1. The external IP shown above should match your proxy server's IP"
echo "2. Check your proxy server logs for incoming connections"
echo "3. All tests above should show ✓ (checkmarks)" 