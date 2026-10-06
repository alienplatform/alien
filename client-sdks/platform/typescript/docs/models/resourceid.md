# ResourceId

## Example Usage

```typescript
import { ResourceId } from "@alienplatform/platform-api/models";

let value: ResourceId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `secretRef`                                                    | [models.ResourceIdSecretRef](../models/resourceidsecretref.md) | :heavy_check_mark:                                             | Reference to a Kubernetes Secret                               |