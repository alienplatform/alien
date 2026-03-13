# Template

## Example Usage

```typescript
import { Template } from "@aliendotdev/platform-api/models/operations";

let value: Template = {
  sourceRepository: "alienplatform/alien",
  forkRepository: "<value>",
  templatePath: "examples/endpoint-agent",
  resolvedRootDirectory: "<value>",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `sourceRepository`                                                                 | [operations.SourceRepository](../../models/operations/sourcerepository.md)         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `forkRepository`                                                                   | *string*                                                                           | :heavy_check_mark:                                                                 | Fork repository in <owner>/<repo> format                                           |
| `templatePath`                                                                     | [operations.TemplatePathResponse](../../models/operations/templatepathresponse.md) | :heavy_check_mark:                                                                 | Template root directory inside alienplatform/alien                                   |
| `resolvedRootDirectory`                                                            | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |