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
    ssoSaml: false,
    auditLogs: true,
    airgapped: true,
  },
  limits: {
    maxDeployments: 1611.31,
    maxProjects: 4036.58,
    maxSeats: 9607.5,
    maxCustomDomains: 9208.96,
    creditUsd: 7075.2,
    seatsIncluded: 7490.77,
  },
  syncedAt: null,
  stale: true,
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