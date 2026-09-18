# GetManagerDeploymentGlobals

## Example Usage

```typescript
import { GetManagerDeploymentGlobals } from "@alienplatform/platform-api/models/operations";

let value: GetManagerDeploymentGlobals = {
  workspace: "my-workspace",
};
```

## Fields

| Field                                                                                                                               | Type                                                                                                                                | Required                                                                                                                            | Description                                                                                                                         | Example                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `workspace`                                                                                                                         | *string*                                                                                                                            | :heavy_minus_sign:                                                                                                                  | Workspace name. Platform API keys already select a workspace; other authentication methods can configure it once on the SDK client. | my-workspace                                                                                                                        |