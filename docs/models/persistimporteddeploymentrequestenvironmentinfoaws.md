# PersistImportedDeploymentRequestEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { PersistImportedDeploymentRequestEnvironmentInfoAws } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                                                                        | Type                                                                                                                                         | Required                                                                                                                                     | Description                                                                                                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `accountId`                                                                                                                                  | *string*                                                                                                                                     | :heavy_check_mark:                                                                                                                           | AWS account ID                                                                                                                               |
| `region`                                                                                                                                     | *string*                                                                                                                                     | :heavy_check_mark:                                                                                                                           | AWS region                                                                                                                                   |
| `platform`                                                                                                                                   | [models.PersistImportedDeploymentRequestEnvironmentInfoPlatformAws](../models/persistimporteddeploymentrequestenvironmentinfoplatformaws.md) | :heavy_check_mark:                                                                                                                           | N/A                                                                                                                                          |