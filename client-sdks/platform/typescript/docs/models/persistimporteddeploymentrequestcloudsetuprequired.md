# PersistImportedDeploymentRequestCloudSetupRequired

A waiting release whose setup change a Helm chart upgrade cannot carry. The
manager works this out from the release and the installed setup, so every
place that offers an upgrade agrees with the manager's report handling.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestCloudSetupRequired } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestCloudSetupRequired = {
  reason: "<value>",
  releaseId: "<id>",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `reason`                                             | *string*                                             | :heavy_check_mark:                                   | Plain-language reason, safe to show to the customer. |
| `releaseId`                                          | *string*                                             | :heavy_check_mark:                                   | The waiting release this applies to.                 |