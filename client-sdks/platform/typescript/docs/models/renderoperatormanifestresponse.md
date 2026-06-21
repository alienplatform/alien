# RenderOperatorManifestResponse

## Example Usage

```typescript
import { RenderOperatorManifestResponse } from "@alienplatform/platform-api/models";

let value: RenderOperatorManifestResponse = {
  manifest: "<value>",
  applyCommand: "<value>",
  filename: "example.file",
  managerUrl: "https://red-citizen.net",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `manifest`                                            | *string*                                              | :heavy_check_mark:                                    | Rendered multi-document Kubernetes manifest           |
| `applyCommand`                                        | *string*                                              | :heavy_check_mark:                                    | kubectl command for applying the manifest from a file |
| `filename`                                            | *string*                                              | :heavy_check_mark:                                    | Suggested local filename                              |
| `managerUrl`                                          | *string*                                              | :heavy_check_mark:                                    | Manager URL embedded in the manifest                  |