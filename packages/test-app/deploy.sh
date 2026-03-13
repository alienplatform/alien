# ===== VENDOR =====

# Build
dotenv -- cargo run --manifest-path=../alien/Cargo.toml --package alien-cli --bin alien -- -C ../alien/packages/test-app build \
    --platform local --targets darwin-arm64


# Release
dotenv -- cargo run --manifest-path=../alien/Cargo.toml --package alien-cli --bin alien -- -C ../alien/packages/test-app \
    --base-url=http://localhost:8080 release


AGENT_ID=$(dotenv -- cargo run --manifest-path=../alien/Cargo.toml --package alien-cli --bin alien -- \
  --base-url=http://localhost:8080 \
  agent create --name=test --project=test-app --platform=local \
  --format=json | jq -r '.id')

echo "Agent created with ID: $AGENT_ID"

# Create agent token for the created agent
dotenv -- cargo run --manifest-path=../alien/Cargo.toml --package alien-cli --bin alien -- agent token \
  --base-url=http://localhost:8080 --id=$AGENT_ID


# ===== "CUSTOMER" ======

```bash
dotenv -- cargo run --manifest-path=../alien/Cargo.toml --package alien-project-cli -- run \
   --name=test --base-url=http://localhost:8080 --token=<agent-token-from-above>
```

