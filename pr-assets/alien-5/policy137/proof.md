# AWS certain-failure policy controls

Real AWS E2E target, eu-west-1, 2026-10-14. Applied Standard On-Demand quota `L-1216C47A`: 16 vCPU. Running/pending On-Demand usage: 0. Manager URL redacted in terminal output. The commands use the normal branded deploy flow with saved compute settings.

| Input | Observed result |
| --- | --- |
| m7i.large, min 0 / max 9 | Visible maximum-capacity warning: demand 18, shortfall 2. Deployment reaches active. |
| m7i.4xlarge, min 2 / max 2 | Immediate demand 32 blocks before owned cloud resources are created. |
| m7i.large, min 0, max 9 → 10 | Update reaches running with current 0 / desired 0 machines; no replacement surge. |
| Same 32-vCPU demand, saved override L-1216C47A | Visible override warning; setup starts. Watchdog stops before ASG/VM creation. This proves policy acceptance and provisioning entry, not successful over-quota fleet deployment. |

The zero-minimum controls avoid unnecessary VM cost and do not prove workload health. Before ALIEN-1506, the successful warning was suppressed at the default branded CLI log level. The same control after the fix shows it. All three successful/partial installations were gracefully destroyed (exit 0). Final provider inventory finds zero owned roles, profiles, ASGs, templates, security groups and instances; no VM launched. See cleanup.json. Existing shared baseline resources were preserved.
