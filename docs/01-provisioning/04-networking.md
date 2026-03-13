# Networking

The `Network` resource manages the VPC/VNet that container clusters run inside. It's auto-created by preflights when any resource requires networking — developers don't declare it directly.

See `01-provisioning/02-preflights.md` for how resources declare network requirements and `01-provisioning/00-infra.md` for the controller/executor model.

## Network Modes

`NetworkSettings` in `StackSettings` controls how networking is provisioned. There are three modes.

### `use-default` — Dev / Fast Bootstrapping

Uses the cloud's existing default network. Designed for fast initial setup — no isolated VPC is created, so there's nothing to wait for or clean up.

VMs get ephemeral public IPs for internet access. Not suitable for production.

| | AWS | GCP | Azure |
|---|---|---|---|
| Network | Default VPC (discovered) | Default network (discovered) | New VNet (created) |
| Subnets | Default public subnets | Default regional subnet | New public + private subnets |
| NAT | None | None | NAT Gateway (always created) |
| Managed by Alien | No | No | Yes |

Azure has no default VNet, so `use-default` creates one. Since Alien is creating infrastructure anyway, it always provisions a NAT Gateway on Azure — making Azure `use-default` behave identically to `create` in practice.

### `create` — Production (Recommended)

Creates an isolated VPC/VNet with NAT. All resources are managed by Alien and deleted when the stack is torn down.

Options:
- `cidr` — CIDR block. Auto-generated from stack ID if not specified to minimize conflicts with other VPCs.
- `availability_zones` (default: 2) — Number of AZs for subnet distribution.

| | AWS | GCP | Azure |
|---|---|---|---|
| Network | New VPC | New VPC (custom mode) | New VNet |
| Subnets | Public + private per AZ | Single regional subnet | Public + private |
| NAT | NAT Gateway + Elastic IP | Cloud Router + Cloud NAT | NAT Gateway + Public IP |
| Security | Security Group | Firewall rules (target tag) | NSG |

VMs use private IPs only. NAT handles all outbound traffic. VMs are never directly reachable from the internet.

### `byo-vpc-*` / `byo-vnet-*` — Customer-Managed Network

References an existing network. Alien validates the references but creates no networking infrastructure.

| Variant | Cloud | Required inputs |
|---|---|---|
| `byo-vpc-aws` | AWS | VPC ID, public subnet IDs, private subnet IDs |
| `byo-vpc-gcp` | GCP | Network name, subnet name, region |
| `byo-vnet-azure` | Azure | VNet resource ID, public subnet name, private subnet name |

The customer is responsible for routing and egress. If VMs cannot reach the internet, that is a customer network configuration issue (their NAT, proxy, VPN, etc.).

## Container Cluster Egress

The container cluster controller configures VMs differently based on the network mode.

```
use-default  →  VMs get ephemeral public IPs (dev convenience)
create       →  VMs use private IPs, NAT handles egress
byo-*        →  no public IPs assigned, customer manages egress
```

The controller reads `network.desired_settings` to determine the mode.

### GCP

The GCP Compute API does not auto-assign external IPs when `accessConfigs` is empty. The controller explicitly adds `AccessConfig { type: ONE_TO_ONE_NAT }` to the instance template only for `use-default`. For `create`, Cloud NAT handles egress and the field is omitted.

### AWS

Subnet selection determines egress. Public subnets auto-assign public IPs via the Internet Gateway. Private subnets route through the NAT Gateway.

- `use-default` → default VPC has no private subnets → public subnets → public IPs
- `create` → `nat_gateway_id` is set → private subnets → NAT egress
- `byo-vpc-aws` → `is_byo_vpc = true` → private subnets → customer-managed egress

### Azure

Azure VMSS networking is `public_ip_address_configuration: None` in all cases.

- `use-default` → Alien creates a NAT Gateway (Azure has no default VNet) → NAT egress
- `create` → Alien creates a NAT Gateway → NAT egress
- `byo-vnet-azure` → customer manages egress, no public IPs from Alien

## Network Resource Lifecycle

The `Network` resource is auto-injected by the `NetworkMutation` preflight when any resource in the stack requires networking. Developers do not declare it.

For `use-default` and `byo-*`, the network controller discovers existing infrastructure and transitions to `Ready` without creating anything (on AWS and GCP). The resources are not tracked for deletion.

For `create`, all provisioned resources are tracked in the controller's state and deleted when the network resource is deleted.
