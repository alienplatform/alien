# ===== VENDOR =====

# Build
dotenv -- cargo run --manifest-path=../alien/Cargo.toml --package alien-cli --bin alien -- -C ../alien/packages/test-app build --platform aws

# Release
dotenv -- cargo run --manifest-path=../alien/Cargo.toml --package alien-cli --bin alien -- -C ../alien/packages/test-app --base-url=http://localhost:8080 release


AGENT_ID=$(dotenv -- cargo run --manifest-path=../alien/Cargo.toml --package alien-cli --bin alien -- \
  --base-url=http://localhost:8080 \
  agent create --name=test --project=test-app --platform=local \
  --format=json | jq -r '.id')

echo "Agent created with ID: $AGENT_ID"

# Create agent token for the created agent
dotenv -- cargo run --manifest-path=../alien/Cargo.toml --package alien-cli --bin alien -- agent token --base-url=http://localhost:8080 --id=$AGENT_ID


# ===== "CUSTOMER" ======

1. Replace `ax_agent_REPLACE_ME` in `main.tf` with actual agent key
2. Build provider: `cargo build --manifest-path=../alien/Cargo.toml -p alien-deploy-cli` (from repo root)
3. Install provider locally:
   ```bash
   # From test-app/terraform directory
   mkdir -p terraform.d/plugins/alien.dev/alien/alien-deploy-cli/1.0.0/darwin_arm64
   cp ../../../target/debug/alien-deploy-cli \
      terraform.d/plugins/alien.dev/alien/alien-deploy-cli/1.0.0/darwin_arm64/terraform-provider-alien-deploy-cli
   chmod +x terraform.d/plugins/alien.dev/alien/alien-deploy-cli/1.0.0/darwin_arm64/terraform-provider-alien-deploy-cli
   ```
   (Note: Change `darwin_arm64` to your platform: `darwin_amd64`, `linux_amd64`, etc.)


## Run

```bash
dotenv -- terraform -chdir=../alien/packages/test-app/terraform init
dotenv -- terraform -chdir=../alien/packages/test-app/terraform apply
```

## Dev Loop

After code changes, rebuild and update the provider:

```bash
cargo build && \
  cp ../../../target/debug/alien-deploy-cli \
     terraform.d/plugins/alien.dev/alien/alien-deploy-cli/1.0.0/darwin_arm64/terraform-provider-alien-deploy-cli && \
  rm -f .terraform.lock.hcl && \
  terraform init && \
  terraform apply
```

Note: We delete `.terraform.lock.hcl` because the provider binary checksum changes after rebuild.

## Debug

```bash
RUST_LOG=debug TF_LOG=DEBUG terraform apply
```

