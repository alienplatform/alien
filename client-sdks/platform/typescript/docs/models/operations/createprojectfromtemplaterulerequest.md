# CreateProjectFromTemplateRuleRequest

## Example Usage

```typescript
import { CreateProjectFromTemplateRuleRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateRuleRequest = {
  apiGroup: "<value>",
  resources: [
    "<value 1>",
  ],
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `apiGroup`                                                                  | *string*                                                                    | :heavy_check_mark:                                                          | Kubernetes API group: empty for core resources, apps, or networking.k8s.io. |
| `resources`                                                                 | *string*[]                                                                  | :heavy_check_mark:                                                          | N/A                                                                         |