# OutputsAgentImage

Outputs from an agent image package build

## Example Usage

```typescript
import { OutputsAgentImage } from "@alienplatform/platform-api/models";

let value: OutputsAgentImage = {
  digest: "<value>",
  image: "https://loremflickr.com/2093/3847?lock=4569584363340966",
  type: "agent-image",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `digest`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | Image digest (e.g., "sha256:abc123...")                                    |
| `image`                                                                    | *string*                                                                   | :heavy_check_mark:                                                         | Full image reference (e.g., "public.ecr.aws/acme/agents/project-id:1.2.3") |
| `type`                                                                     | [models.OutputsTypeAgentImage](../models/outputstypeagentimage.md)         | :heavy_check_mark:                                                         | N/A                                                                        |