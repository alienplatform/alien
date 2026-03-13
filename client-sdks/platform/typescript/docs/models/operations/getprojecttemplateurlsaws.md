# GetProjectTemplateUrlsAws

Template URLs for deploying an AWS agent

## Example Usage

```typescript
import { GetProjectTemplateUrlsAws } from "@aliendotdev/platform-api/models/operations";

let value: GetProjectTemplateUrlsAws = {
  templateUrl: "https://competent-cheese.name/",
  launchStackUrl: "https://gorgeous-goodwill.info/",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `templateUrl`                                                | *string*                                                     | :heavy_check_mark:                                           | URL to download the CloudFormation template                  |
| `launchStackUrl`                                             | *string*                                                     | :heavy_check_mark:                                           | URL to launch the template in the AWS CloudFormation console |