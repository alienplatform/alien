# GetProjectProtectedSetupReleaseRequest

## Example Usage

```typescript
import { GetProjectProtectedSetupReleaseRequest } from "@alienplatform/platform-api/models/operations";

let value: GetProjectProtectedSetupReleaseRequest = {
  idOrName: "<value>",
  platform: "local",
  channel: "<value>",
  setupTarget: "<value>",
  setupFingerprint: "<value>",
  setupFingerprintVersion: 515225,
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                               | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Project ID or name.                                                                                                      |
| `platform`                                                                                                               | [operations.GetProjectProtectedSetupReleasePlatform](../../models/operations/getprojectprotectedsetupreleaseplatform.md) | :heavy_check_mark:                                                                                                       | Represents the target cloud platform.                                                                                    |
| `channel`                                                                                                                | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | The channel the setup pinned                                                                                             |
| `setupTarget`                                                                                                            | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Stable target key for the setup contract, e.g. aws/us-east-1                                                             |
| `setupFingerprint`                                                                                                       | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Deterministic setup contract fingerprint for one setup target                                                            |
| `setupFingerprintVersion`                                                                                                | *number*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |