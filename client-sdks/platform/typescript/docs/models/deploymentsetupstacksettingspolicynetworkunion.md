# DeploymentSetupStackSettingsPolicyNetworkUnion


## Supported Types

### `models.DeploymentSetupStackSettingsPolicyNetworkUseDefault`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyNetworkUseDefault = {
  type: "use-default",
};
```

### `models.DeploymentSetupStackSettingsPolicyNetworkCreate`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyNetworkCreate = {
  type: "create",
};
```

### `models.DeploymentSetupStackSettingsPolicyNetworkByoVpcAws`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyNetworkByoVpcAws = {
  privateSubnetIds: [],
  publicSubnetIds: [
    "<value 1>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.DeploymentSetupStackSettingsPolicyNetworkByoVpcGcp`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.DeploymentSetupStackSettingsPolicyNetworkByoVnetAzure`

```typescript
const value: models.DeploymentSetupStackSettingsPolicyNetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

