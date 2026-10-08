# WorkspaceBillingEntitlements

## Example Usage

```typescript
import { WorkspaceBillingEntitlements } from "@alienplatform/platform-api/models";

let value: WorkspaceBillingEntitlements = {
  planId: "pro_annual",
  planStatus: "past_due",
  features: {
    customDomains: true,
    privateManagers: false,
    operationsCustomPlugins: false,
    ssoSaml: true,
    auditLogs: true,
    airgapped: true,
  },
  limits: {
    maxDeployments: 1385.32,
    maxProjects: 3144.64,
    maxSeats: 3743.19,
    maxCustomDomains: 7182.98,
    creditUsd: 2481.03,
    seatsIncluded: 863.33,
  },
  syncedAt: new Date("2025-01-06T04:12:40.643Z"),
  stale: false,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `planId`                                                                                      | [models.PlanId](../models/planid.md)                                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `planStatus`                                                                                  | [models.BillingPlanStatus](../models/billingplanstatus.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `features`                                                                                    | [models.BillingFeatureFlags](../models/billingfeatureflags.md)                                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `limits`                                                                                      | [models.BillingLimits](../models/billinglimits.md)                                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `syncedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `stale`                                                                                       | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |