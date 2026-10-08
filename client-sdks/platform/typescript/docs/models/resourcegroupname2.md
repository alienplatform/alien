# ResourceGroupName2

## Example Usage

```typescript
import { ResourceGroupName2 } from "@alienplatform/platform-api/models";

let value: ResourceGroupName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `secretRef`                                                                    | [models.ResourceGroupNameSecretRef2](../models/resourcegroupnamesecretref2.md) | :heavy_check_mark:                                                             | Reference to a Kubernetes Secret                                               |
