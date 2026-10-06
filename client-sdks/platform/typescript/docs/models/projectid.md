# ProjectId

## Example Usage

```typescript
import { ProjectId } from "@alienplatform/platform-api/models";

let value: ProjectId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.ProjectIdSecretRef](../models/projectidsecretref.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |