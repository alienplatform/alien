# SlackIntegrationInstallUrlRequest

## Example Usage

```typescript
import { SlackIntegrationInstallUrlRequest } from "@alienplatform/platform-api/models/operations";

let value: SlackIntegrationInstallUrlRequest = {
  workspace: "my-workspace",
};
```

## Fields

| Field                                                                                                                                                                                  | Type                                                                                                                                                                                   | Required                                                                                                                                                                               | Description                                                                                                                                                                            | Example                                                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace`                                                                                                                                                                            | *string*                                                                                                                                                                               | :heavy_minus_sign:                                                                                                                                                                     | Workspace name. Required for user/session/OAuth requests. Optional for API keys because API keys are workspace-scoped; if provided with an API key, it must match the key's workspace. | my-workspace                                                                                                                                                                           |
