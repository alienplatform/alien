# BillingFeatureFlags

## Example Usage

```typescript
import { BillingFeatureFlags } from "@alienplatform/platform-api/models";

let value: BillingFeatureFlags = {
  customDomains: true,
  privateManagers: true,
  operationsCustomPlugins: false,
  ssoSaml: false,
  auditLogs: false,
  airgapped: true,
};
```

## Fields

| Field                     | Type                      | Required                  | Description               |
| ------------------------- | ------------------------- | ------------------------- | ------------------------- |
| `customDomains`           | *boolean*                 | :heavy_check_mark:        | N/A                       |
| `privateManagers`         | *boolean*                 | :heavy_check_mark:        | N/A                       |
| `operationsCustomPlugins` | *boolean*                 | :heavy_check_mark:        | N/A                       |
| `ssoSaml`                 | *boolean*                 | :heavy_check_mark:        | N/A                       |
| `auditLogs`               | *boolean*                 | :heavy_check_mark:        | N/A                       |
| `airgapped`               | *boolean*                 | :heavy_check_mark:        | N/A                       |