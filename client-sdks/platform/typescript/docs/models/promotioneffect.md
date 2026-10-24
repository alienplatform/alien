# PromotionEffect

Filter deployments by the effect of promoting this release to their channel

## Example Usage

```typescript
import { PromotionEffect } from "@alienplatform/platform-api/models";

let value: PromotionEffect = "retries";
```

## Values

```typescript
"updates" | "unconfirmed" | "retries" | "already-on" | "pinned" | "customer-applies" | "waits-for-approval" | "waits-for-owner" | "waits-for-inputs" | "not-affected"
```