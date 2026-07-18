# SyncAcquireResponseDeploymentHorizondArtifacts

Download artifact for one horizond release platform.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentHorizondArtifacts } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentHorizondArtifacts = {
  sha256: "<value>",
  url: "https://faint-wombat.org",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `sha256`                                 | *string*                                 | :heavy_check_mark:                       | SHA-256 digest for the artifact payload. |
| `url`                                    | *string*                                 | :heavy_check_mark:                       | HTTPS URL for the artifact.              |