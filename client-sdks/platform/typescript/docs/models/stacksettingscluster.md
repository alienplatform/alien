# StackSettingsCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { StackSettingsCluster } from "@alienplatform/platform-api/models";

let value: StackSettingsCluster = {
  ownership: "external",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `cloud`                                                              | *models.StackSettingsCloudUnion*                                     | :heavy_minus_sign:                                                   | N/A                                                                  |
| `namespace`                                                          | *string*                                                             | :heavy_minus_sign:                                                   | Namespace where the Alien chart and application resources run.       |
| `ownership`                                                          | [models.StackSettingsOwnership](../models/stacksettingsownership.md) | :heavy_check_mark:                                                   | Ownership model for the Kubernetes cluster.                          |