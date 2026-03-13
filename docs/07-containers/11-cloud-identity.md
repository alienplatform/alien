# Cloud Identity

How containers running on VMs get cloud credentials transparently.

## The Problem

Containers scheduled by Horizon run on VMs in the customer's cloud account. These containers need cloud credentials to access resources like S3 buckets, SQS queues, or other cloud services — but they don't run directly on a cloud compute service (like Lambda or ECS) that provides credentials natively.

Alien solves this with three capabilities:

1. **IMDS metadata proxy** — transparently vends cloud credentials to containers
2. **Cross-account image pull** — authenticates with container registries in the managing account
3. **Per-container service account impersonation** — each container gets its own cloud identity

## IMDS Metadata Proxy

horizond runs an HTTP proxy that intercepts the cloud metadata endpoint (`169.254.169.254`). Every container on the VM believes it's talking to the standard cloud metadata service, but horizond intercepts the request and returns credentials specific to that container.

### How It Works

1. **DNAT redirect** — An nftables rule redirects all traffic from the container bridge network destined for `169.254.169.254:80` to horizond's metadata proxy on port 15556:

   ```
   iifname "br-appnet0" ip daddr 169.254.169.254 tcp dport 80 dnat to {container_gateway}:15556
   ```

2. **Container identification** — horizond identifies which container is making the request by its source IP address. Each container gets a unique IP on the bridge network, and horizond maintains a mapping from IP to container name and service account.

3. **Credential vending** — Based on the container's assigned service account, horizond fetches and returns the appropriate cloud credentials.

### Per-Cloud Implementation

**AWS (IMDSv2):**
- horizond issues IMDSv2 tokens (6-hour TTL, capped at 21600 seconds per spec)
- Token→container mapping tracked for subsequent credential requests
- Credentials fetched via `sts:AssumeRole` for the container's IAM role
- Expired tokens evicted opportunistically

**GCP:**
- Responds to `/computeMetadata/v1/instance/service-accounts/default/token`
- Calls `iamcredentials:generateAccessToken` for the container's service account
- Returns access tokens with 5-minute refresh buffer before expiration

**Azure:**
- Responds to `/metadata/identity/oauth2/token`
- Fetches managed identity tokens for the container's assigned client ID
- Cache key: `(client_id, resource)` tuple with 5-minute refresh buffer

### Caching

horizond caches credentials to avoid excessive API calls:

```rust
aws_cred_cache: RwLock<HashMap<String, CachedAwsCredentials>>,
gcp_token_cache: RwLock<HashMap<String, CachedGcpToken>>,
azure_token_cache: RwLock<HashMap<(String, String), CachedAzureToken>>,
```

## Cross-Account Image Pull

Container images are often stored in the managing account's registry (not the customer's account). horizond authenticates with the registry before pulling images, using the VM's own cloud identity.

### Registry Detection

horizond detects the registry type from the image hostname:

| Pattern | Registry |
|---------|----------|
| `*.dkr.ecr.*.amazonaws.com/*` | AWS ECR |
| `*-docker.pkg.dev/*` | GCP Artifact Registry |
| `*.azurecr.io/*` | Azure ACR |
| Everything else | Public (Docker Hub, ghcr.io, etc.) |

### Authentication Per Cloud

**AWS ECR:**
- Calls `ecr:GetAuthorizationToken()` using VM's credentials
- Returns base64-encoded `AWS:password` pair
- Cached for ~11.5 hours (12-hour token minus 30-minute buffer)

**GCP Artifact Registry:**
- Gets VM's access token from the real metadata server
- Uses `oauth2accesstoken` as username with the access token as password

**Azure ACR:**
- Gets VM's managed identity token for `management.azure.com`
- Exchanges MI token for ACR refresh token via `POST /oauth2/exchange`
- Uses a fixed UUID as username with the refresh token as password

## Per-Container Service Account Impersonation

Each container can have its own cloud identity. When a container's permission profile maps to a ServiceAccount, horizond vends credentials specific to that service account — not the VM's identity.

### How the Mapping Gets to horizond

1. During deployment, Alien resolves each container's permission profile to a ServiceAccount (IAM Role ARN, GCP SA email, or Azure MI client ID).
2. Horizon receives the service account mapping in the deployment config.
3. On each heartbeat, horizond receives `ReplicaAssignment` data that includes the service account target for each container replica.
4. horizond stores this in its cluster state:

```rust
pub enum ContainerServiceAccount {
    Aws { role_arn: String },
    Gcp { email: String },
    Azure { client_id: String },
}
```

### Credential Vending Per Cloud

**AWS:** Container requests `GET /latest/meta-data/iam/security-credentials/{role-name}`. horizond calls `sts:AssumeRole` with the container's role ARN using the VM's credentials. Returns temporary credentials (access key, secret key, session token).

**GCP:** Container requests `GET /computeMetadata/v1/instance/service-accounts/default/token`. horizond calls `iamcredentials:generateAccessToken` with the container's SA email. Returns an access token scoped to that service account.

**Azure:** Container requests `GET /metadata/identity/oauth2/token?resource=...`. horizond calls Azure IMDS with the container's `client_id` to get a token for that specific user-assigned managed identity.

From the container's perspective, it's using standard cloud SDKs with no special configuration. The IMDS proxy is completely transparent.

## Trust Policy Extensions

For the VM to impersonate container service accounts, the trust policies on those service accounts must allow it.

The ServiceAccount controller in `alien-infra` builds trust policies by analyzing the stack. When a Container uses a permission profile, the controller adds the ContainerCluster's VM role to the trust policy:

**AWS:**
```json
{
  "Effect": "Allow",
  "Principal": {
    "AWS": "arn:aws:iam::{account}:role/{prefix}-{cluster}-role"
  },
  "Action": "sts:AssumeRole"
}
```
The VM role ARN is added alongside any other principals (Lambda, CodeBuild, other service accounts that impersonate this one).

**GCP:** Grants `iam.serviceAccounts.getAccessToken` permission to the VM's service account on the target SA.

**Azure:** Grants managed identity token permission to the VM's identity on the target MI.

Implementation: `alien/crates/alien-infra/src/service_account/aws.rs` (trust policy generation logic)

## The `container-cluster/execute` Permission Set

The `container-cluster/execute` permission set grants the VM identity the permissions needed for both cross-account image pull and per-container SA impersonation. It's automatically included in the management permission profile when the stack contains a ContainerCluster.

**What it grants per cloud:**

| Cloud | Image Pull | SA Impersonation |
|-------|-----------|------------------|
| **AWS** | `ecr:BatchGetImage`, `ecr:GetDownloadUrlForLayer`, `ecr:GetAuthorizationToken` on managing account ECR | `sts:AssumeRole` on `{prefix}-*-sa` roles |
| **GCP** | `artifactregistry.repositories.downloadArtifacts` on managing project | `iam.serviceAccounts.getAccessToken` on project SAs |
| **Azure** | `Microsoft.ContainerRegistry/registries/pull/read` on managing subscription ACR | `Microsoft.ManagedIdentity/userAssignedIdentities/token/action` on subscription MIs |

Permission set definition: `alien/crates/alien-permissions/permission-sets/container-cluster/execute.jsonc`
