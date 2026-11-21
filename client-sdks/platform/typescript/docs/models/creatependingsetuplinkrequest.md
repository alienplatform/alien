# CreatePendingSetupLinkRequest

## Example Usage

```typescript
import { CreatePendingSetupLinkRequest } from "@alienplatform/platform-api/models";

let value: CreatePendingSetupLinkRequest = {
  slots: [
    "<value 1>",
  ],
};
```

## Fields

| Field                                                                                                                                                   | Type                                                                                                                                                    | Required                                                                                                                                                | Description                                                                                                                                             |
| ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `slots`                                                                                                                                                 | *string*[]                                                                                                                                              | :heavy_check_mark:                                                                                                                                      | The env:NAME and input:ID slots the client will write to the link's manager.                                                                            |
| `platforms`                                                                                                                                             | [models.CreatePendingSetupLinkRequestPlatform](../models/creatependingsetuplinkrequestplatform.md)[]                                                    | :heavy_minus_sign:                                                                                                                                      | The platforms the link will allow. Picks the manager that holds the values. Omit for every platform of the latest release.                              |
| `releaseChannel`                                                                                                                                        | *string*                                                                                                                                                | :heavy_minus_sign:                                                                                                                                      | The release channel the link's deployment item selects. Slots are checked against that channel's active release. Omit for the project's latest release. |