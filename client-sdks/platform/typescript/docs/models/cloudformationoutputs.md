# CloudformationOutputs

Outputs from a CloudFormation package build.

## Example Usage

```typescript
import { CloudformationOutputs } from "@alienplatform/platform-api/models";

let value: CloudformationOutputs = {
  targets: {
    "key": {
      launchStackUrl: "https://dramatic-produce.org",
      sha256: "<value>",
      size: 327718,
      stackPolicyUrl: "https://ideal-fold.name/",
      target: "<value>",
      templateUrl: "https://insecure-creator.biz/",
    },
  },
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `targets`                                                                          | Record<string, [models.CloudformationTargets](../models/cloudformationtargets.md)> | :heavy_check_mark:                                                                 | Template artifacts by CloudFormation target.                                       |