#!/bin/bash

echo "🧪 Testing Firecracker POC API..."

# Test health endpoint
echo "📊 Testing health endpoint..."
curl -s http://localhost:3000/health
echo ""

# Test execute endpoint with simple Python code
echo "🐍 Testing execute endpoint with simple Python code..."
curl -X POST http://localhost:3000/execute \
  -H 'Content-Type: application/json' \
  -d '{"code": "print(2 + 2)"}'
echo ""

# Test execute endpoint with more complex Python code
echo "🐍 Testing execute endpoint with complex Python code..."
curl -X POST http://localhost:3000/execute \
  -H 'Content-Type: application/json' \
  -d '{"code": "import math\nresult = math.sqrt(16)\nprint(f\"Square root of 16 is {result}\")"}'
echo ""

# Test error handling with invalid code
echo "❌ Testing error handling with invalid Python code..."
curl -X POST http://localhost:3000/execute \
  -H 'Content-Type: application/json' \
  -d '{"code": "print(undefined_variable)"}'
echo ""

echo "✅ API testing complete!"
