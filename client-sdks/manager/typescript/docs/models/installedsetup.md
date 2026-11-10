# InstalledSetup

The setup an installed chart was built for, read from the Operator's own
environment. It changes only when the chart's setup changes.

## Example Usage

```typescript
import { InstalledSetup } from "@alienplatform/manager-api/models";

let value: InstalledSetup = {
  releaseId: "<id>",
  setupFingerprint: "<value>",
  setupFingerprintVersion: 761967,
  setupTarget: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `namespace`                                            | *string*                                               | :heavy_minus_sign:                                     | Kubernetes namespace of the Helm release.              |
| `releaseId`                                            | *string*                                               | :heavy_check_mark:                                     | Release the chart was built from.                      |
| `releaseName`                                          | *string*                                               | :heavy_minus_sign:                                     | Helm release name the chart is installed as.           |
| `setupFingerprint`                                     | *string*                                               | :heavy_check_mark:                                     | Setup compatibility fingerprint embedded in the chart. |
| `setupFingerprintVersion`                              | *number*                                               | :heavy_check_mark:                                     | Fingerprint algorithm version.                         |
| `setupTarget`                                          | *string*                                               | :heavy_check_mark:                                     | Setup target the chart was generated for.              |
