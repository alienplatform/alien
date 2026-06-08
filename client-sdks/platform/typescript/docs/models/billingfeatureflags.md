# BillingFeatureFlags

## Example Usage

```typescript
import { BillingFeatureFlags } from "@alienplatform/platform-api/models";

let value: BillingFeatureFlags = {
  customDomains: true,
  privateManagers: true,
  ssoSaml: false,
  auditLogs: false,
  airgapped: false,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `customDomains`    | *boolean*          | :heavy_check_mark: | N/A                |
| `privateManagers`  | *boolean*          | :heavy_check_mark: | N/A                |
| `ssoSaml`          | *boolean*          | :heavy_check_mark: | N/A                |
| `auditLogs`        | *boolean*          | :heavy_check_mark: | N/A                |
| `airgapped`        | *boolean*          | :heavy_check_mark: | N/A                |