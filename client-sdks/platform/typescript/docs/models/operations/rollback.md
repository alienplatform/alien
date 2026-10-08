# Rollback

The release the deployment reported before it moved to the desired release

## Example Usage

```typescript
import { Rollback } from "@alienplatform/platform-api/models/operations";

let value: Rollback = {
  releaseId: "<id>",
  version: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `releaseId`        | *string*           | :heavy_check_mark: | N/A                |
| `version`          | *string*           | :heavy_check_mark: | N/A                |