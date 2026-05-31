# SyncAcquireResponseBaseImage

Base image metadata for the Horizon machine image.

## Example Usage

```typescript
import { SyncAcquireResponseBaseImage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseBaseImage = {
  name: "<value>",
  version: "<value>",
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `name`                            | *string*                          | :heavy_check_mark:                | Base OS image name.               |
| `version`                         | *string*                          | :heavy_check_mark:                | Base OS image version or channel. |