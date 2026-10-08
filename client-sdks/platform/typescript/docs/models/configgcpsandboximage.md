# ConfigGcpSandboxImage

Configuration for a GCP sandbox image package.

The image is the developer's base with the sandbox agent and its contract layered on, pushed
into the project's path of the GCP manager's registry.

## Example Usage

```typescript
import { ConfigGcpSandboxImage } from "@alienplatform/platform-api/models";

let value: ConfigGcpSandboxImage = {
  agentImage: "<value>",
  baseImage: "<value>",
  customImage: "<value>",
  managerUrl: "https://stark-subexpression.com",
  type: "gcp-sandbox-image",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `agentImage`                                                                           | *string*                                                                               | :heavy_check_mark:                                                                     | Full reference of the published sandbox agent image the agent binary is read from.     |
| `baseImage`                                                                            | *string*                                                                               | :heavy_check_mark:                                                                     | `custom_image` resolved to its linux/amd64 manifest, `registry/repository@sha256:...`. |
| `customImage`                                                                          | *string*                                                                               | :heavy_check_mark:                                                                     | The developer's image as typed, which ties the package to the capability that asked.   |
| `managerUrl`                                                                           | *string*                                                                               | :heavy_check_mark:                                                                     | URL of the GCP manager whose registry proxy receives the push.                         |
| `type`                                                                                 | *"gcp-sandbox-image"*                                                                  | :heavy_check_mark:                                                                     | N/A                                                                                    |