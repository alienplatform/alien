import { readFile, readdir, writeFile } from "node:fs/promises";

// Speakeasy may regroup inline models when optional fields are added. Preserve
// published deep import paths while using the current generated response parsers.
const modules = {
  "createmanagerresponseproviderawsalb2": "createmanagerresponseprovidergkegatewayenum2",
  "createmanagerresponsedomainsunion2": "createmanagerresponsepublicendpointtargetunion2",
  "deploymentdetailresponsependingpreparedstacktypeunion": "deploymentdetailresponsependingpreparedstacktypeenvenum",
  "deploymentdetailresponsepreparedstackoperationsunion": "deploymentdetailresponsepreparedstackoperations",
  "deploymentpendingpreparedstacktypeunion": "deploymentpendingpreparedstacktypeenvenum",
  "deploymentpreparedstackoperationsunion": "deploymentpreparedstackoperations",
  "managerretryresponseendpointaccess2": "managerretryresponsedomains2",
  "managerretryresponseproviderunion2": "managerretryresponseprovidergkegateway2",
  "persistimporteddeploymentrequestpendingpreparedstackcustomsettingsunion": "persistimporteddeploymentrequestpendingpreparedstacksettingscustom1",
  "persistimporteddeploymentrequestpreparedstackoverridestackconditionunion": "persistimporteddeploymentrequestpreparedstackoverrideconditionstack",
  "synclistresponsependingpreparedstacktypeunion": "synclistresponsependingpreparedstacktypeenvenum",
  "synclistresponsepreparedstackoperationsunion": "synclistresponsepreparedstackoperations",
  "targetdeploymentconfig": "targetdeploymentvolumerestore",
  "releaseinfotypestringlist": "targetdeploymentconfig"
};
// Names can move separately from the shared models in their former module.
const additionalExports = {
  "targetdeploymentconfig": {
    "targetdeployment": [
      "TargetDeploymentConfig",
      "TargetDeploymentConfig$inboundSchema",
      "targetDeploymentConfigFromJSON"
]
  },
  "createmanagerresponseproviderawsalb2": {
    "createmanagerresponse": [
      "CreateManagerResponseProviderAwsAlb2",
      "CreateManagerResponseProviderAwsAlb2$inboundSchema",
      "CreateManagerResponseProviderAwsAlbEnum2",
      "CreateManagerResponseProviderAwsAlbEnum2$inboundSchema",
      "CreateManagerResponseProviderGkeGateway2",
      "CreateManagerResponseProviderGkeGateway2$inboundSchema",
      "createManagerResponseProviderAwsAlb2FromJSON",
      "createManagerResponseProviderGkeGateway2FromJSON"
    ]
  },
  "createmanagerresponsedomainsunion2": {
    "createmanagerresponseprovidergkegatewayenum2": [
      "CreateManagerResponseDomains2",
      "CreateManagerResponseDomains2$inboundSchema",
      "CreateManagerResponseDomainsUnion2",
      "CreateManagerResponseDomainsUnion2$inboundSchema",
      "createManagerResponseDomains2FromJSON",
      "createManagerResponseDomainsUnion2FromJSON"
    ]
  },
  "deploymentdetailresponsependingpreparedstacktypeunion": {
    "deploymentdetailresponsepreparedstackoperations": [
      "DeploymentDetailResponsePendingPreparedStackTypeUnion",
      "DeploymentDetailResponsePendingPreparedStackTypeUnion$inboundSchema",
      "deploymentDetailResponsePendingPreparedStackTypeUnionFromJSON"
    ]
  },
  "deploymentdetailresponsepreparedstackoperationsunion": {
    "deploymentdetailresponse": [
      "DeploymentDetailResponsePreparedStackOperationsUnion",
      "DeploymentDetailResponsePreparedStackOperationsUnion$inboundSchema",
      "deploymentDetailResponsePreparedStackOperationsUnionFromJSON"
    ]
  },
  "deploymentpendingpreparedstacktypeunion": {
    "deploymentpreparedstackoperations": [
      "DeploymentPendingPreparedStackTypeUnion",
      "DeploymentPendingPreparedStackTypeUnion$inboundSchema",
      "deploymentPendingPreparedStackTypeUnionFromJSON"
    ]
  },
  "deploymentpreparedstackoperationsunion": {
    "deployment": [
      "DeploymentPreparedStackOperationsUnion",
      "DeploymentPreparedStackOperationsUnion$inboundSchema",
      "deploymentPreparedStackOperationsUnionFromJSON"
    ]
  },
  "managerretryresponseendpointaccess2": {
    "managerretryresponseprovidergkegateway2": [
      "ManagerRetryResponseDomainsUnion2",
      "ManagerRetryResponseDomainsUnion2$inboundSchema",
      "ManagerRetryResponseEndpointAccess2",
      "ManagerRetryResponseEndpointAccess2$inboundSchema",
      "managerRetryResponseDomainsUnion2FromJSON"
    ]
  },
  "managerretryresponseproviderunion2": {
    "managerretryresponse": [
      "ManagerRetryResponseProviderAwsAlb2",
      "ManagerRetryResponseProviderAwsAlb2$inboundSchema",
      "ManagerRetryResponseProviderAwsAlbEnum2",
      "ManagerRetryResponseProviderAwsAlbEnum2$inboundSchema",
      "ManagerRetryResponseProviderUnion2",
      "ManagerRetryResponseProviderUnion2$inboundSchema",
      "managerRetryResponseProviderAwsAlb2FromJSON",
      "managerRetryResponseProviderUnion2FromJSON"
    ]
  },
  "persistimporteddeploymentrequestpendingpreparedstackcustomsettingsunion": {
    "persistimporteddeploymentrequestpreparedstackoverrideconditionstack": [
      "PersistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnion",
      "PersistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnion$Outbound",
      "PersistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnion$outboundSchema",
      "persistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnionToJSON"
    ]
  },
  "persistimporteddeploymentrequestpreparedstackoverridestackconditionunion": {
    "persistimporteddeploymentrequest": [
      "PersistImportedDeploymentRequestPreparedStackOverrideStackConditionUnion",
      "PersistImportedDeploymentRequestPreparedStackOverrideStackConditionUnion$Outbound",
      "PersistImportedDeploymentRequestPreparedStackOverrideStackConditionUnion$outboundSchema",
      "persistImportedDeploymentRequestPreparedStackOverrideStackConditionUnionToJSON"
    ]
  },
  "synclistresponsependingpreparedstacktypeunion": {
    "synclistresponsepreparedstackoperations": [
      "SyncListResponsePendingPreparedStackTypeUnion",
      "SyncListResponsePendingPreparedStackTypeUnion$inboundSchema",
      "syncListResponsePendingPreparedStackTypeUnionFromJSON"
    ]
  },
  "synclistresponsepreparedstackoperationsunion": {
    "synclistresponse": [
      "SyncListResponsePreparedStackOperationsUnion",
      "SyncListResponsePreparedStackOperationsUnion$inboundSchema",
      "syncListResponsePreparedStackOperationsUnionFromJSON"
    ]
  },
  "releaseinfotypestringlist": {
    "targetdeployment": [
      "ReleaseInfoTypeStringList",
      "ReleaseInfoTypeStringList$inboundSchema"
    ]
  }
};
// Authority settings add fields to inline groups; retain their published module paths.
Object.assign(modules, {
  "createmanagerresponseprovidergkegatewayenum2": "createmanagerresponseproviderazureapplicationgatewayforcontainersenum2",
  "createmanagerresponsepublicendpointtargetunion2": "createmanagerresponsepublicendpointtargetmachineaddresses2",
  "deploymentdetailresponsependingpreparedstacktypeenvenum": "deploymentdetailresponsependingpreparedstackdefaultunion",
  "deploymentdetailresponsepreparedstackoperations": "deploymentdetailresponsepreparedstackplugins",
  "deploymentpendingpreparedstacktypeenvenum": "deploymentpendingpreparedstackdefaultunion",
  "deploymentpreparedstackoperations": "deploymentpreparedstackplugins",
  "managerretryresponseprovidergkegateway2": "managerretryresponseproviderazureapplicationgatewayforcontainers2",
  "managerretryresponsedomains2": "managerretryresponsepublicendpointtargetunion2",
  "persistimporteddeploymentrequestpendingpreparedstacksettingscustom1": "persistimporteddeploymentrequestpendingpreparedstacksettingscustom2",
  "persistimporteddeploymentrequestpreparedstackoverrideconditionstack": "persistimporteddeploymentrequestpreparedstackoverridegcpresource",
  "synclistresponsependingpreparedstacktypeenvenum": "synclistresponsependingpreparedstackdefaultunion",
  "synclistresponsepreparedstackoperations": "synclistresponsepreparedstackplugins"
});
Object.assign(additionalExports, {
  "createmanagerresponseprovidergkegatewayenum2": {
    "createmanagerresponse": [
      "CreateManagerResponseProviderAzureApplicationGatewayForContainers2",
      "CreateManagerResponseProviderAzureApplicationGatewayForContainers2$inboundSchema",
      "CreateManagerResponseProviderGkeGatewayEnum2",
      "CreateManagerResponseProviderGkeGatewayEnum2$inboundSchema",
      "createManagerResponseProviderAzureApplicationGatewayForContainers2FromJSON"
    ]
  },
  "createmanagerresponsepublicendpointtargetunion2": {
    "createmanagerresponseproviderazureapplicationgatewayforcontainersenum2": [
      "CreateManagerResponsePublicEndpointTargetUnion2",
      "CreateManagerResponsePublicEndpointTargetUnion2$inboundSchema",
      "createManagerResponsePublicEndpointTargetUnion2FromJSON"
    ]
  },
  "deploymentdetailresponsependingpreparedstacktypeenvenum": {
    "deploymentdetailresponsepreparedstackplugins": [
      "DeploymentDetailResponsePendingPreparedStackTypeEnvEnum",
      "DeploymentDetailResponsePendingPreparedStackTypeEnvEnum$inboundSchema"
    ]
  },
  "deploymentdetailresponsepreparedstackoperations": {
    "deploymentdetailresponse": [
      "DeploymentDetailResponsePreparedStackOperations",
      "DeploymentDetailResponsePreparedStackOperations$inboundSchema",
      "deploymentDetailResponsePreparedStackOperationsFromJSON"
    ]
  },
  "deploymentpendingpreparedstacktypeenvenum": {
    "deploymentpreparedstackplugins": [
      "DeploymentPendingPreparedStackTypeEnvEnum",
      "DeploymentPendingPreparedStackTypeEnvEnum$inboundSchema"
    ]
  },
  "deploymentpreparedstackoperations": {
    "deployment": [
      "DeploymentPreparedStackOperations",
      "DeploymentPreparedStackOperations$inboundSchema",
      "deploymentPreparedStackOperationsFromJSON"
    ]
  },
  "managerretryresponseprovidergkegateway2": {
    "managerretryresponse": [
      "ManagerRetryResponseProviderGkeGateway2",
      "ManagerRetryResponseProviderGkeGateway2$inboundSchema",
      "ManagerRetryResponseProviderGkeGatewayEnum2",
      "ManagerRetryResponseProviderGkeGatewayEnum2$inboundSchema",
      "managerRetryResponseProviderGkeGateway2FromJSON"
    ]
  },
  "managerretryresponsedomains2": {
    "managerretryresponseproviderazureapplicationgatewayforcontainers2": [
      "ManagerRetryResponseDomains2",
      "ManagerRetryResponseDomains2$inboundSchema",
      "managerRetryResponseDomains2FromJSON"
    ]
  },
  "persistimporteddeploymentrequestpendingpreparedstacksettingscustom1": {
    "persistimporteddeploymentrequestpreparedstackoverridegcpresource": [
      "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom1",
      "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom1$Outbound",
      "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom1$outboundSchema",
      "persistImportedDeploymentRequestPendingPreparedStackSettingsCustom1ToJSON"
    ]
  },
  "persistimporteddeploymentrequestpreparedstackoverrideconditionstack": {
    "persistimporteddeploymentrequest": [
      "PersistImportedDeploymentRequestPreparedStackOverrideConditionStack",
      "PersistImportedDeploymentRequestPreparedStackOverrideConditionStack$Outbound",
      "PersistImportedDeploymentRequestPreparedStackOverrideConditionStack$outboundSchema",
      "persistImportedDeploymentRequestPreparedStackOverrideConditionStackToJSON"
    ]
  },
  "synclistresponsependingpreparedstacktypeenvenum": {
    "synclistresponsepreparedstackplugins": [
      "SyncListResponsePendingPreparedStackTypeEnvEnum",
      "SyncListResponsePendingPreparedStackTypeEnvEnum$inboundSchema"
    ]
  },
  "synclistresponsepreparedstackoperations": {
    "synclistresponse": [
      "SyncListResponsePreparedStackOperations",
      "SyncListResponsePreparedStackOperations$inboundSchema",
      "syncListResponsePreparedStackOperationsFromJSON"
    ]
  }
});
// Federation adds setup-management metadata to the shared target configuration.
// Preserve the published volume-restore module when its shared declarations move.
modules.targetdeploymentvolumerestore = "targetdeploymentsetupmanagement";
additionalExports.targetdeploymentvolumerestore = {
  targetdeployment: [
    "TargetDeploymentStackSettings", "TargetDeploymentStackSettings$inboundSchema",
    "TargetDeploymentTelemetry", "TargetDeploymentTelemetry$inboundSchema",
    "TargetDeploymentUpdates", "TargetDeploymentUpdates$inboundSchema",
    "TargetDeploymentVolumeRestore", "TargetDeploymentVolumeRestore$inboundSchema",
    "targetDeploymentStackSettingsFromJSON", "targetDeploymentVolumeRestoreFromJSON"
  ]
};
// Retain the previously published paths after shared inline declarations regroup.
Object.assign(modules, {
  "deploymentdetailresponsependingpreparedstackdefaultunion": "deploymentdetailresponsependingpreparedstackdefaultstringlist",
  "deploymentdetailresponsepreparedstackplugins": "deploymentdetailresponsepreparedstackapprovalplugins",
  "deploymentpendingpreparedstackdefaultunion": "deploymentpendingpreparedstackdefaultstringlist",
  "deploymentpreparedstackplugins": "deploymentpreparedstackapprovalplugins",
  "synclistresponsependingpreparedstackdefaultunion": "synclistresponsependingpreparedstackdefaultstringlist",
  "synclistresponsepreparedstackplugins": "synclistresponsepreparedstackapprovalplugins",
  "persistimporteddeploymentrequestpreparedstackoverridegcpresource": "persistimporteddeploymentrequestpreparedstackoverrideazureresource"
});
modules.persistimporteddeploymentrequestpendingpreparedstacksettingscustom2 = "persistimporteddeploymentrequestpendingpreparedstackvalidationunion";
additionalExports.persistimporteddeploymentrequestpendingpreparedstacksettingscustom2 = {
  "persistimporteddeploymentrequestpreparedstackoverrideazureresource": [
    "PersistImportedDeploymentRequestPendingPreparedStackApprovalCustom",
    "PersistImportedDeploymentRequestPendingPreparedStackApprovalCustom$Outbound",
    "PersistImportedDeploymentRequestPendingPreparedStackApprovalCustom$outboundSchema",
    "PersistImportedDeploymentRequestPendingPreparedStackApprovalCustomEnum",
    "PersistImportedDeploymentRequestPendingPreparedStackApprovalCustomEnum$outboundSchema",
    "PersistImportedDeploymentRequestPendingPreparedStackCustomApprovalUnion",
    "PersistImportedDeploymentRequestPendingPreparedStackCustomApprovalUnion$Outbound",
    "PersistImportedDeploymentRequestPendingPreparedStackCustomApprovalUnion$outboundSchema",
    "PersistImportedDeploymentRequestPendingPreparedStackCustomDecision",
    "PersistImportedDeploymentRequestPendingPreparedStackCustomDecision$outboundSchema",
    "PersistImportedDeploymentRequestPendingPreparedStackInput",
    "PersistImportedDeploymentRequestPendingPreparedStackInput$Outbound",
    "PersistImportedDeploymentRequestPendingPreparedStackInput$outboundSchema",
    "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom2",
    "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom2$Outbound",
    "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom2$outboundSchema",
    "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom3",
    "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom3$Outbound",
    "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom3$outboundSchema",
    "persistImportedDeploymentRequestPendingPreparedStackApprovalCustomToJSON",
    "persistImportedDeploymentRequestPendingPreparedStackCustomApprovalUnionToJSON",
    "persistImportedDeploymentRequestPendingPreparedStackInputToJSON",
    "persistImportedDeploymentRequestPendingPreparedStackSettingsCustom2ToJSON",
    "persistImportedDeploymentRequestPendingPreparedStackSettingsCustom3ToJSON"
  ]
};
// Stable deployment-state model paths.
Object.assign(modules, {
  "deploymentstatependingpreparedstackdefaultunion": "deploymentstatependingpreparedstackdefaultstringlist",
  "deploymentstatepreparedstackplugins": "deploymentstatepreparedstackapprovalplugins",
  "targetreleaseoperations": "targetreleaseapprovalpluginsenum",
  "typekv4": "keyprefixsecretref1"
});
Object.assign(additionalExports, {
  "deploymentstatependingpreparedstackdefaultunion": {
    "deploymentstatepreparedstackapprovalplugins": [
      "DeploymentStatePendingPreparedStackDefaultBoolean",
      "DeploymentStatePendingPreparedStackDefaultBoolean$Outbound",
      "DeploymentStatePendingPreparedStackDefaultBoolean$inboundSchema",
      "DeploymentStatePendingPreparedStackDefaultBoolean$outboundSchema",
      "DeploymentStatePendingPreparedStackDefaultNumber",
      "DeploymentStatePendingPreparedStackDefaultNumber$Outbound",
      "DeploymentStatePendingPreparedStackDefaultNumber$inboundSchema",
      "DeploymentStatePendingPreparedStackDefaultNumber$outboundSchema",
      "DeploymentStatePendingPreparedStackDefaultString",
      "DeploymentStatePendingPreparedStackDefaultString$Outbound",
      "DeploymentStatePendingPreparedStackDefaultString$inboundSchema",
      "DeploymentStatePendingPreparedStackDefaultString$outboundSchema",
      "DeploymentStatePendingPreparedStackDefaultUnion",
      "DeploymentStatePendingPreparedStackDefaultUnion$Outbound",
      "DeploymentStatePendingPreparedStackDefaultUnion$inboundSchema",
      "DeploymentStatePendingPreparedStackDefaultUnion$outboundSchema",
      "DeploymentStatePendingPreparedStackTypeBoolean",
      "DeploymentStatePendingPreparedStackTypeBoolean$inboundSchema",
      "DeploymentStatePendingPreparedStackTypeBoolean$outboundSchema",
      "DeploymentStatePendingPreparedStackTypeNumber",
      "DeploymentStatePendingPreparedStackTypeNumber$inboundSchema",
      "DeploymentStatePendingPreparedStackTypeNumber$outboundSchema",
      "DeploymentStatePendingPreparedStackTypeString",
      "DeploymentStatePendingPreparedStackTypeString$inboundSchema",
      "DeploymentStatePendingPreparedStackTypeString$outboundSchema",
      "deploymentStatePendingPreparedStackDefaultBooleanFromJSON",
      "deploymentStatePendingPreparedStackDefaultBooleanToJSON",
      "deploymentStatePendingPreparedStackDefaultNumberFromJSON",
      "deploymentStatePendingPreparedStackDefaultNumberToJSON",
      "deploymentStatePendingPreparedStackDefaultStringFromJSON",
      "deploymentStatePendingPreparedStackDefaultStringToJSON",
      "deploymentStatePendingPreparedStackDefaultUnionFromJSON",
      "deploymentStatePendingPreparedStackDefaultUnionToJSON"
    ]
  },
  "deploymentstatepreparedstackplugins": {
    "targetreleaseapprovalpluginsenum": [
      "DeploymentStatePreparedStackApprovalPluginsEnum",
      "DeploymentStatePreparedStackApprovalPluginsEnum$inboundSchema",
      "DeploymentStatePreparedStackApprovalPluginsEnum$outboundSchema",
      "DeploymentStatePreparedStackPlugins",
      "DeploymentStatePreparedStackPlugins$Outbound",
      "DeploymentStatePreparedStackPlugins$inboundSchema",
      "DeploymentStatePreparedStackPlugins$outboundSchema",
      "DeploymentStatePreparedStackPluginsApprovalUnion",
      "DeploymentStatePreparedStackPluginsApprovalUnion$Outbound",
      "DeploymentStatePreparedStackPluginsApprovalUnion$inboundSchema",
      "DeploymentStatePreparedStackPluginsApprovalUnion$outboundSchema",
      "DeploymentStatePreparedStackPluginsSettingsUnion",
      "DeploymentStatePreparedStackPluginsSettingsUnion$Outbound",
      "DeploymentStatePreparedStackPluginsSettingsUnion$inboundSchema",
      "DeploymentStatePreparedStackPluginsSettingsUnion$outboundSchema",
      "DeploymentStatePreparedStackSettingsPlugins1",
      "DeploymentStatePreparedStackSettingsPlugins1$Outbound",
      "DeploymentStatePreparedStackSettingsPlugins1$inboundSchema",
      "DeploymentStatePreparedStackSettingsPlugins1$outboundSchema",
      "DeploymentStatePreparedStackSettingsPlugins2",
      "DeploymentStatePreparedStackSettingsPlugins2$Outbound",
      "DeploymentStatePreparedStackSettingsPlugins2$inboundSchema",
      "DeploymentStatePreparedStackSettingsPlugins2$outboundSchema",
      "DeploymentStatePreparedStackSettingsPlugins3",
      "DeploymentStatePreparedStackSettingsPlugins3$Outbound",
      "DeploymentStatePreparedStackSettingsPlugins3$inboundSchema",
      "DeploymentStatePreparedStackSettingsPlugins3$outboundSchema",
      "deploymentStatePreparedStackPluginsApprovalUnionFromJSON",
      "deploymentStatePreparedStackPluginsApprovalUnionToJSON",
      "deploymentStatePreparedStackPluginsFromJSON",
      "deploymentStatePreparedStackPluginsSettingsUnionFromJSON",
      "deploymentStatePreparedStackPluginsSettingsUnionToJSON",
      "deploymentStatePreparedStackPluginsToJSON",
      "deploymentStatePreparedStackSettingsPlugins1FromJSON",
      "deploymentStatePreparedStackSettingsPlugins1ToJSON",
      "deploymentStatePreparedStackSettingsPlugins2FromJSON",
      "deploymentStatePreparedStackSettingsPlugins2ToJSON",
      "deploymentStatePreparedStackSettingsPlugins3FromJSON",
      "deploymentStatePreparedStackSettingsPlugins3ToJSON"
    ]
  },
  "targetreleaseoperations": {
    "deploymentstate": [
      "TargetReleaseOperations",
      "TargetReleaseOperations$Outbound",
      "TargetReleaseOperations$inboundSchema",
      "TargetReleaseOperations$outboundSchema",
      "TargetReleasePlugins",
      "TargetReleasePlugins$Outbound",
      "TargetReleasePlugins$inboundSchema",
      "TargetReleasePlugins$outboundSchema",
      "TargetReleasePluginsApprovalUnion",
      "TargetReleasePluginsApprovalUnion$Outbound",
      "TargetReleasePluginsApprovalUnion$inboundSchema",
      "TargetReleasePluginsApprovalUnion$outboundSchema",
      "TargetReleasePluginsSettingsUnion",
      "TargetReleasePluginsSettingsUnion$Outbound",
      "TargetReleasePluginsSettingsUnion$inboundSchema",
      "TargetReleasePluginsSettingsUnion$outboundSchema",
      "TargetReleaseSettingsPlugins1",
      "TargetReleaseSettingsPlugins1$Outbound",
      "TargetReleaseSettingsPlugins1$inboundSchema",
      "TargetReleaseSettingsPlugins1$outboundSchema",
      "TargetReleaseSettingsPlugins2",
      "TargetReleaseSettingsPlugins2$Outbound",
      "TargetReleaseSettingsPlugins2$inboundSchema",
      "TargetReleaseSettingsPlugins2$outboundSchema",
      "TargetReleaseSettingsPlugins3",
      "TargetReleaseSettingsPlugins3$Outbound",
      "TargetReleaseSettingsPlugins3$inboundSchema",
      "TargetReleaseSettingsPlugins3$outboundSchema",
      "targetReleaseOperationsFromJSON",
      "targetReleaseOperationsToJSON",
      "targetReleasePluginsApprovalUnionFromJSON",
      "targetReleasePluginsApprovalUnionToJSON",
      "targetReleasePluginsFromJSON",
      "targetReleasePluginsSettingsUnionFromJSON",
      "targetReleasePluginsSettingsUnionToJSON",
      "targetReleasePluginsToJSON",
      "targetReleaseSettingsPlugins1FromJSON",
      "targetReleaseSettingsPlugins1ToJSON",
      "targetReleaseSettingsPlugins2FromJSON",
      "targetReleaseSettingsPlugins2ToJSON",
      "targetReleaseSettingsPlugins3FromJSON",
      "targetReleaseSettingsPlugins3ToJSON"
    ]
  },
  "typekv4": {
    "externalbindingunion": [
      "KeyPrefix1",
      "KeyPrefix1$Outbound",
      "KeyPrefix1$inboundSchema",
      "KeyPrefix1$outboundSchema",
      "TypeKv4",
      "TypeKv4$inboundSchema",
      "TypeKv4$outboundSchema",
      "keyPrefix1FromJSON",
      "keyPrefix1ToJSON"
    ]
  }
});
const deploymentStatusUrl = new URL("../typescript/src/models/deploymentstatependingpreparedstackdefaultstringlist.ts", import.meta.url);
const deploymentStatusMarker = "\n// Published deployment-state readiness exports.\n";
const deploymentStatusSource = (await readFile(deploymentStatusUrl, "utf8")).split(deploymentStatusMarker)[0];
await writeFile(deploymentStatusUrl, `${deploymentStatusSource}${deploymentStatusMarker}export const DeploymentStateRuntimeMetadataStatus = DeploymentStateDeployerSecretStatus;
export type DeploymentStateRuntimeMetadataStatus = DeploymentStateDeployerSecretStatus;
export const DeploymentStateRuntimeMetadataStatus$inboundSchema = DeploymentStateDeployerSecretStatus$inboundSchema;
export const DeploymentStateRuntimeMetadataStatus$outboundSchema = DeploymentStateDeployerSecretStatus$outboundSchema;
`);
const models = new URL("../typescript/src/models/", import.meta.url);
// Preserve the published secret-readiness enum after its shared owner changes.
const readinessUrl = new URL("persistimporteddeploymentrequestpendingpreparedstackvalidationunion.ts", models);
const readinessMarker = "\n// Published secret-readiness compatibility exports.\n";
const readinessSource = (await readFile(readinessUrl, "utf8")).split(readinessMarker)[0];
await writeFile(readinessUrl, `${readinessSource}${readinessMarker}export const PersistImportedDeploymentRequestRuntimeMetadataStatus = PersistImportedDeploymentRequestDeployerSecretStatus;
export type PersistImportedDeploymentRequestRuntimeMetadataStatus = PersistImportedDeploymentRequestDeployerSecretStatus;
export const PersistImportedDeploymentRequestRuntimeMetadataStatus$outboundSchema = PersistImportedDeploymentRequestDeployerSecretStatus$outboundSchema;
`);
async function exportIsValue(module, name) {
  if (modules[module]) {
    const moved = Object.entries(additionalExports[module] ?? {})
      .find(([, names]) => names.includes(name));
    return exportIsValue(moved?.[0] ?? modules[module], name);
  }
  const source = await readFile(new URL(`${module}.ts`, models), "utf8");
  return [...source.matchAll(/export\s+(?:const|function|class)\s+([\w$]+)/g)]
    .some(match => match[1] === name);
}

// Generate leaf forwarders before older compatibility paths that reference them.
const orderedModules = Object.entries(modules).sort(([, left], [, right]) =>
  Number(left in modules) - Number(right in modules));
for (const [previous, current] of orderedModules) {
  const existing = await readFile(new URL(`${previous}.ts`, models), "utf8").catch(() => "");
  if (existing && !existing.startsWith("// Generated by generate-compatibility-exports.mjs.")) continue;
  await readFile(new URL(`${current}.ts`, models));
  const reexports = [];
  for (const [module, names] of Object.entries(additionalExports[previous] ?? {})) {
    for (const name of names) {
      const value = await exportIsValue(module, name);
      reexports.push(`export ${value ? "" : "type "}{ ${name} } from "./${module}.js";`);
    }
  }
  await writeFile(
    new URL(`${previous}.ts`, models),
    `// Generated by generate-compatibility-exports.mjs. DO NOT EDIT.\nexport * from "./${current}.js";\n${reexports.join("\n")}\n`.trimEnd() + "\n",
  );
}

// Preserve serializers and types reported by older runtimes when the generator
// qualifies their names with the enclosing request. Root and deep imports both
// continue to expose the existing names.
const resourceMarker = "\n// Published resource-name compatibility exports.\n";
for (const file of await readdir(models)) {
  if (!file.endsWith(".ts")) continue;
  const url = new URL(file, models);
  const original = await readFile(url, "utf8");
  const source = original.split(resourceMarker)[0];
  const declarations = [...source.matchAll(/export\s+(type|const|function)\s+((?:SyncReconcileRequest(?:CpuUnion|MemoryUnion|Container)\d+(?:\$Outbound|\$outboundSchema)?|syncReconcileRequest(?:CpuUnion|MemoryUnion|Container)\d+ToJSON))(?![\w$])/g)];
  const aliases = declarations.map(([, kind, name]) => {
    const alias = name.startsWith("SyncReconcileRequest")
      ? name.slice("SyncReconcileRequest".length)
      : name.slice("syncReconcileRequest".length).replace(/^[A-Z]/, value => value.toLowerCase());
    return `export ${kind === "type" ? "type " : ""}{ ${name} as ${alias} };`;
  });
  if (aliases.length) await writeFile(url, `${source}${resourceMarker}${aliases.join("\n")}\n`);
}

// Inline setup settings share enums with deployment info after regeneration.
// Preserve the public deployment-info names at both root and deep import paths.
const deploymentInfoUrl = new URL("deploymentinfo.ts", models);
const deploymentInfoMarker = "\n// Published deployment-info compatibility exports.\n";
const deploymentInfoSource = (await readFile(deploymentInfoUrl, "utf8")).split(deploymentInfoMarker)[0];
const deploymentInfoAliases = ["Heartbeats", "Telemetry", "Updates"].flatMap(name => [
  `export { SetupUpdate${name} as DeploymentInfo${name} };`,
  `export { SetupUpdate${name}$inboundSchema as DeploymentInfo${name}$inboundSchema };`,
]);
await writeFile(deploymentInfoUrl, `${deploymentInfoSource}${deploymentInfoMarker}${deploymentInfoAliases.join("\n")}\n`);

// A second inline Kubernetes secret endpoint numbers the existing external
// binding endpoint models. Preserve their published names at both root and deep
// import paths.
const externalBindingUrl = new URL("externalbindingunion.ts", models);
const externalBindingMarker = "\n// Published external-binding endpoint compatibility exports.\n";
const externalBindingSource = (await readFile(externalBindingUrl, "utf8")).split(externalBindingMarker)[0];
const externalBindingAliases = ["EndpointSecretRef", "ExternalBindingEndpoint"].flatMap(name => {
  const serializer = name[0].toLowerCase() + name.slice(1);
  return [
    `export type { ${name}1 as ${name} };`,
    `export type { ${name}1$Outbound as ${name}$Outbound };`,
    `export { ${name}1$inboundSchema as ${name}$inboundSchema };`,
    `export { ${name}1$outboundSchema as ${name}$outboundSchema };`,
    `export { ${serializer}1FromJSON as ${serializer}FromJSON };`,
    `export { ${serializer}1ToJSON as ${serializer}ToJSON };`,
  ];
});
await writeFile(externalBindingUrl, `${externalBindingSource}${externalBindingMarker}${externalBindingAliases.join("\n")}\n`);

// The access-request risk enum is named `MaxRisk` once no other inline
// `maxRisk` enum competes for the name. Preserve its published name.
const accessRequestUrl = new URL("createaccessrequest.ts", models);
const accessRequestMarker = "\n// Published access-request risk compatibility exports.\n";
const accessRequestSource = (await readFile(accessRequestUrl, "utf8")).split(accessRequestMarker)[0];
const accessRequestAliases = /export const MaxRisk =/.test(accessRequestSource)
  ? `${accessRequestMarker}export const CreateAccessRequestMaxRisk = MaxRisk;
export type CreateAccessRequestMaxRisk = MaxRisk;
export const CreateAccessRequestMaxRisk$outboundSchema = MaxRisk$outboundSchema;
`
  : `${accessRequestMarker}export const MaxRisk = CreateAccessRequestMaxRisk;
export type MaxRisk = CreateAccessRequestMaxRisk;
export const MaxRisk$outboundSchema = CreateAccessRequestMaxRisk$outboundSchema;
`;
await writeFile(accessRequestUrl, `${accessRequestSource}${accessRequestAliases}`);

// Reservation command variants disambiguate the preparation response models.
// Preserve their published names, parsers and deep imports.
const preparationUrl = new URL("deploymentpreparationresponse.ts", models);
const preparationMarker = "\n// Published preparation compatibility exports.\n";
const preparationSource = (await readFile(preparationUrl, "utf8")).split(preparationMarker)[0];
const preparationAliases = ["Linux", "Mac", "CommandVariants"].flatMap(name => {
  const parser = name[0].toLowerCase() + name.slice(1);
  return [
    `export type { DeploymentPreparationResponse${name} as ${name} };`,
    `export { DeploymentPreparationResponse${name}$inboundSchema as ${name}$inboundSchema };`,
    `export { deploymentPreparationResponse${name}FromJSON as ${parser}FromJSON };`,
  ];
});
await writeFile(preparationUrl, `${preparationSource}${preparationMarker}${preparationAliases.join("\n")}\n`);
