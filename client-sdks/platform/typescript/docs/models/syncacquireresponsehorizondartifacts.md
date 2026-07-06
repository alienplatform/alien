# SyncAcquireResponseHorizondArtifacts

Downloadable horizond daemon artifact.

## Example Usage

```typescript
import { SyncAcquireResponseHorizondArtifacts } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizondArtifacts = {
  sha256: "<value>",
  url: "https://separate-tinderbox.info/",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `sha256`                           | *string*                           | :heavy_check_mark:                 | Expected artifact sha256 checksum. |
| `url`                              | *string*                           | :heavy_check_mark:                 | Artifact URL.                      |