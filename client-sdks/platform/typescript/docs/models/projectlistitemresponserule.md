# ProjectListItemResponseRule

## Example Usage

```typescript
import { ProjectListItemResponseRule } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseRule = {
  apiGroup: "<value>",
  resources: [],
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `apiGroup`                                                                  | *string*                                                                    | :heavy_check_mark:                                                          | Kubernetes API group: empty for core resources, apps, or networking.k8s.io. |
| `resources`                                                                 | *string*[]                                                                  | :heavy_check_mark:                                                          | N/A                                                                         |