#pragma once
#include <windows.h>
#include <string>
#include <vector>

namespace AetherShell::Settings
{
    struct IpcResult
    {
        bool success{ false };
        std::string payload;
        std::string error;
    };

    class SettingsIpcClient
    {
    public:
        static std::wstring ResolvePipeName();
        static std::string LoadSessionToken();
        static IpcResult SendCommand(const std::string& command, const std::string& jsonPayload = "{}");
    };
}
