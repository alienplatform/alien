# StackSettingsNetworkUnion

## Supported Types

### `models.StackSettingsNetworkUseDefault`

```typescript
const value: models.StackSettingsNetworkUseDefault = {
  type: "use-default",
};
```

### `models.StackSettingsNetworkCreate`

```typescript
const value: models.StackSettingsNetworkCreate = {
  type: "create",
};
```

### `models.StackSettingsNetworkByoVpcAws`

```typescript
const value: models.StackSettingsNetworkByoVpcAws = {
  privateSubnetIds: ["<value 1>", "<value 2>", "<value 3>"],
  publicSubnetIds: ["<value 1>"],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.StackSettingsNetworkByoVpcGcp`

```typescript
const value: models.StackSettingsNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.StackSettingsNetworkByoVnetAzure`

```typescript
const value: models.StackSettingsNetworkByoVnetAzure = {
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
