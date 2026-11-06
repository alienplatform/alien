# CreateFirstPartyDeploymentSessionRequest

## Example Usage

```typescript
import { CreateFirstPartyDeploymentSessionRequest } from "@alienplatform/platform-api/models";

let value: CreateFirstPartyDeploymentSessionRequest = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `setupItems`                                                                                                                 | [models.CreateFirstPartyDeploymentSessionRequestSetupItem](../models/createfirstpartydeploymentsessionrequestsetupitem.md)[] | :heavy_minus_sign:                                                                                                           | What to install: the application and the features beside it. Omit for the application alone.                                 |