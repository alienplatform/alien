# TargetDeploymentHorizondArtifacts

Download artifact for one horizond release platform.

## Example Usage

```typescript
import { TargetDeploymentHorizondArtifacts } from "@alienplatform/platform-api/models";

let value: TargetDeploymentHorizondArtifacts = {
  sha256: "<value>",
  url: "https://aged-following.name",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `sha256`                                 | *string*                                 | :heavy_check_mark:                       | SHA-256 digest for the artifact payload. |
| `url`                                    | *string*                                 | :heavy_check_mark:                       | HTTPS URL for the artifact.              |