# CreateProjectFromTemplateTemplate

## Example Usage

```typescript
import { CreateProjectFromTemplateTemplate } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateTemplate = {
  sourceRepository: "alienplatform/alien",
  forkRepository: "<value>",
  templatePath: "examples/remote-worker-ts",
  resolvedRootDirectory: "<value>",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `sourceRepository`                                                                                                                   | [operations.CreateProjectFromTemplateSourceRepository](../../models/operations/createprojectfromtemplatesourcerepository.md)         | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
| `forkRepository`                                                                                                                     | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | Fork repository in <owner>/<repo> format                                                                                             |
| `templatePath`                                                                                                                       | [operations.CreateProjectFromTemplateTemplatePathResponse](../../models/operations/createprojectfromtemplatetemplatepathresponse.md) | :heavy_check_mark:                                                                                                                   | Template root directory inside alienplatform/alien                                                                                   |
| `resolvedRootDirectory`                                                                                                              | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |