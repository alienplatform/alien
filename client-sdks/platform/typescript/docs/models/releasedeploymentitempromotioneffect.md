# ReleaseDeploymentItemPromotionEffect

What promoting this release to the requested `channel` does to the deployment. Present only when the request names a channel.

## Example Usage

```typescript
import { ReleaseDeploymentItemPromotionEffect } from "@alienplatform/platform-api/models";

let value: ReleaseDeploymentItemPromotionEffect = "unconfirmed";
```

## Values

```typescript
"updates" | "unconfirmed" | "retries" | "already-on" | "pinned" | "customer-applies" | "waits-for-approval" | "waits-for-owner" | "waits-for-inputs" | "not-affected"
```