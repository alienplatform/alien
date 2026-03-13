# OutputsOperatorImage

Outputs from an operator image package build

## Example Usage

```typescript
import { OutputsOperatorImage } from "@aliendotdev/platform-api/models";

let value: OutputsOperatorImage = {
  digest: "<value>",
  image: "https://loremflickr.com/104/2323?lock=152100383342186",
  type: "operator-image",
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `digest`                                                                      | *string*                                                                      | :heavy_check_mark:                                                            | Image digest (e.g., "sha256:abc123...")                                       |
| `image`                                                                       | *string*                                                                      | :heavy_check_mark:                                                            | Full image reference (e.g., "public.ecr.aws/acme/operators/project-id:1.2.3") |
| `type`                                                                        | [models.OutputsTypeOperatorImage](../models/outputstypeoperatorimage.md)      | :heavy_check_mark:                                                            | N/A                                                                           |