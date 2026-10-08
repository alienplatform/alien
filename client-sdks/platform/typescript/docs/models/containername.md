# ContainerName

## Example Usage

```typescript
import { ContainerName } from "@alienplatform/platform-api/models";

let value: ContainerName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `secretRef`                                                          | [models.ContainerNameSecretRef](../models/containernamesecretref.md) | :heavy_check_mark:                                                   | Reference to a Kubernetes Secret                                     |
