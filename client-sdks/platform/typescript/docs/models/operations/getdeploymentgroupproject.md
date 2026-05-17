# GetDeploymentGroupProject

Project info, included when ?include=project is used

## Example Usage

```typescript
import { GetDeploymentGroupProject } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentGroupProject = {
  id: "<id>",
  name: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `id`               | *string*           | :heavy_check_mark: | Project ID         |
| `name`             | *string*           | :heavy_check_mark: | Project name       |