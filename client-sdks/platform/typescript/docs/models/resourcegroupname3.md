# ResourceGroupName3

## Example Usage

```typescript
import { ResourceGroupName3 } from "@alienplatform/platform-api/models";

let value: ResourceGroupName3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `secretRef`                                                                    | [models.ResourceGroupNameSecretRef3](../models/resourcegroupnamesecretref3.md) | :heavy_check_mark:                                                             | Reference to a Kubernetes Secret                                               |