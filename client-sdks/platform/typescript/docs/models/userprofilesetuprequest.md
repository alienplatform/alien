# UserProfileSetupRequest

## Example Usage

```typescript
import { UserProfileSetupRequest } from "@alienplatform/platform-api/models";

let value: UserProfileSetupRequest = {
  name: "<value>",
  company: "Cremin, Parisian and Jast",
  acquisitionSource: "founder",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                   | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Display name                                                                                             |
| `company`                                                                                                | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Company name                                                                                             |
| `acquisitionSource`                                                                                      | [models.UserProfileSetupRequestAcquisitionSource](../models/userprofilesetuprequestacquisitionsource.md) | :heavy_check_mark:                                                                                       | How the user heard about Alien                                                                           |
| `acquisitionSourceDetail`                                                                                | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | Required when acquisitionSource is other                                                                 |
| `useCaseTags`                                                                                            | [models.UserProfileSetupRequestUseCaseTag](../models/userprofilesetuprequestusecasetag.md)[]             | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `useCases`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |