# TargetDeploymentClustersHorizondArtifacts

Machine agent download for one release platform.

## Example Usage

```typescript
import { TargetDeploymentClustersHorizondArtifacts } from "@alienplatform/platform-api/models";

let value: TargetDeploymentClustersHorizondArtifacts = {
  sha256: "<value>",
  url: "https://voluminous-swim.org/",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `runtimeIsolationGeneration`                                                           | *number*                                                                               | :heavy_minus_sign:                                                                     | Runtime isolation capability generation of the immutable artifact, not live readiness. |
| `sha256`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | SHA-256 digest for the artifact payload.                                               |
| `url`                                                                                  | *string*                                                                               | :heavy_check_mark:                                                                     | HTTPS URL for the artifact.                                                            |