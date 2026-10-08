# TemplateRequest

## Example Usage

```typescript
import { TemplateRequest } from "@alienplatform/platform-api/models/operations";

let value: TemplateRequest = {
  mode: "template",
  targetNamespace: "<value>",
  templatePath: "examples/customer-models-ts",
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `mode`                                                                                                                             | *"template"*                                                                                                                       | :heavy_check_mark:                                                                                                                 | N/A                                                                                                                                |
| `targetNamespace`                                                                                                                  | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | N/A                                                                                                                                |
| `templatePath`                                                                                                                     | [operations.ConfigureProjectSourceTemplatePathRequest](../../models/operations/configureprojectsourcetemplatepathrequest.md)       | :heavy_check_mark:                                                                                                                 | Template root directory inside alienplatform/alien                                                                                 |
| `rootDirectory`                                                                                                                    | *string*                                                                                                                           | :heavy_minus_sign:                                                                                                                 | The name of a directory or relative path to the source code of your project. When null is used it will default to the project root |