// HelloWorldPakServer.Target.cs — dedicated-server target. This is
// the target we cook against (`-targetplatform=WindowsServer` in
// the cook command in scripts/helloworld-pak/cook.ps1) because
// SCUMServer.exe is a dedicated-server build of UE4. Cooking for
// Server vs Game changes which assets get included and the asset
// reference graph; the Server target excludes editor-only and
// client-only assets that the dedicated server doesn't need.

using UnrealBuildTool;
using System.Collections.Generic;

public class HelloWorldPakServerTarget : TargetRules
{
	public HelloWorldPakServerTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Server;
		DefaultBuildSettings = BuildSettingsVersion.V2;
		ExtraModuleNames.AddRange(new string[] { "HelloWorldPak" });
	}
}
