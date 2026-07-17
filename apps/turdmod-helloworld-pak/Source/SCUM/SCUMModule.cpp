// SCUM stub module implementation. FDefaultModuleImpl is enough — no custom startup;
// the module exists solely so /Script/SCUM and its stub vehicle classes are registered
// + loadable in the editor (LoadingPhase Default), letting BPs parent to /Script/SCUM.Airplane.
#include "Modules/ModuleManager.h"

IMPLEMENT_MODULE(FDefaultModuleImpl, SCUM);
