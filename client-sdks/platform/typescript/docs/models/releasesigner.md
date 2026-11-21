# ReleaseSigner

## Example Usage

```typescript
import { ReleaseSigner } from "@alienplatform/platform-api/models";

let value: ReleaseSigner = {
  id: "<id>",
  label: "<value>",
  kind: "device",
};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `id`                                                    | *string*                                                | :heavy_check_mark:                                      | N/A                                                     |
| `label`                                                 | *string*                                                | :heavy_check_mark:                                      | Label from the trusted-device list, such as `alice-mbp` |
| `kind`                                                  | [models.ReleaseKind](../models/releasekind.md)          | :heavy_check_mark:                                      | N/A                                                     |