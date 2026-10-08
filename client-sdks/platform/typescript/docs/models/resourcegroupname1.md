# ResourceGroupName1

## Example Usage

```typescript
import { ResourceGroupName1 } from "@alienplatform/platform-api/models";

let value: ResourceGroupName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `secretRef`                                                                    | [models.ResourceGroupNameSecretRef1](../models/resourcegroupnamesecretref1.md) | :heavy_check_mark:                                                             | Reference to a Kubernetes Secret                                               |