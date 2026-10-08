# OutputsGcpSandboxImage

Outputs from a GCP sandbox image package build.

## Example Usage

```typescript
import { OutputsGcpSandboxImage } from "@alienplatform/platform-api/models";

let value: OutputsGcpSandboxImage = {
  agentDigest: "<value>",
  baseDigest: "<value>",
  digest: "<value>",
  image: "https://picsum.photos/seed/IYDQUAdZ5e/827/188",
  reuseKey: "<value>",
  type: "gcp-sandbox-image",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `agentDigest`                                                                                | *string*                                                                                     | :heavy_check_mark:                                                                           | Manifest digest of the linux/amd64 agent image the agent binary came from.                   |
| `baseDigest`                                                                                 | *string*                                                                                     | :heavy_check_mark:                                                                           | Manifest digest of the linux/amd64 base the image was built from.                            |
| `digest`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | Manifest digest of the pushed image.                                                         |
| `image`                                                                                      | *string*                                                                                     | :heavy_check_mark:                                                                           | Native registry reference pinned by digest, the value a GCP sandbox stack names.             |
| `reuseKey`                                                                                   | *string*                                                                                     | :heavy_check_mark:                                                                           | Hash of the base digest, the agent image reference and the image contract; equal keys reuse. |
| `type`                                                                                       | [models.OutputsTypeGcpSandboxImage](../models/outputstypegcpsandboximage.md)                 | :heavy_check_mark:                                                                           | N/A                                                                                          |
