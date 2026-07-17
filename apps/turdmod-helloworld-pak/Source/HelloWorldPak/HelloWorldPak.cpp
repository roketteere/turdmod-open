// HelloWorldPak.cpp — module init for the Phase C helloworld pak.
// IMPLEMENT_PRIMARY_GAME_MODULE registers the module with UE's
// module manager so cooking targets a real game module rather than
// a plain library.

#include "HelloWorldPak.h"
#include "Modules/ModuleManager.h"

IMPLEMENT_PRIMARY_GAME_MODULE(FDefaultGameModuleImpl, HelloWorldPak, "HelloWorldPak");
