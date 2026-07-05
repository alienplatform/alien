# SyncAcquireResponseHorizondArtifacts

Download artifact for one horizond release platform.

## Example Usage

```typescript
import { SyncAcquireResponseHorizondArtifacts } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizondArtifacts = {
  sha256: "<value>",
  url: "https://separate-tinderbox.info/",
};
```

## Fields

| Field    | Type     | Required           | Description                              |
| -------- | -------- | ------------------ | ---------------------------------------- |
| `sha256` | _string_ | :heavy_check_mark: | SHA-256 digest for the artifact payload. |
| `url`    | _string_ | :heavy_check_mark: | HTTPS URL for the artifact.              |
