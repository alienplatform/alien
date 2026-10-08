# DeploymentConfigHorizondArtifacts

Download artifact for one horizond release platform.

## Example Usage

```typescript
import { DeploymentConfigHorizondArtifacts } from "@alienplatform/platform-api/models";

let value: DeploymentConfigHorizondArtifacts = {
  sha256: "<value>",
  url: "https://utter-leading.com/",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `runtimeIsolationGeneration`                                                           | *number*                                                                               | :heavy_minus_sign:                                                                     | Runtime isolation capability generation of the immutable artifact, not live readiness. |
| `sha256`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | SHA-256 digest for the artifact payload.                                               |
| `url`                                                                                  | *string*                                                                               | :heavy_check_mark:                                                                     | HTTPS URL for the artifact.                                                            |
