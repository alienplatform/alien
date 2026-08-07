# ConfigureProjectSourceTemplateResponse

## Example Usage

```typescript
import { ConfigureProjectSourceTemplateResponse } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceTemplateResponse = {
  sourceRepository: "alienplatform/alien",
  forkRepository: "<value>",
  templatePath: "examples/customer-models-ts",
  resolvedRootDirectory: "<value>",
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `sourceRepository`                                                                                                             | [operations.ConfigureProjectSourceSourceRepository](../../models/operations/configureprojectsourcesourcerepository.md)         | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |
| `forkRepository`                                                                                                               | *string*                                                                                                                       | :heavy_check_mark:                                                                                                             | Fork repository in <owner>/<repo> format                                                                                       |
| `templatePath`                                                                                                                 | [operations.ConfigureProjectSourceTemplatePathResponse](../../models/operations/configureprojectsourcetemplatepathresponse.md) | :heavy_check_mark:                                                                                                             | Template root directory inside alienplatform/alien                                                                             |
| `resolvedRootDirectory`                                                                                                        | *string*                                                                                                                       | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |