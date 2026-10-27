#pragma once
#include <windows.h>
#include <string>
#include <vector>

namespace AetherShell::Settings
{
    struct IpcResult
    {
        bool transportSuccess{ false };
        bool success{ false };
        std::string payload;
        std::string error;
    };

    class SettingsIpcClient
    {
    public:
        static std::wstring ResolvePipeName(const std::wstring& pipeOverride = L"");
        static std::string LoadSessionToken(const std::wstring& tokenPathOverride = L"");
        static IpcResult SendCommand(
            const std::string& command,
            const std::string& jsonPayload = "{}",
            const std::wstring& pipeOverride = L"",
            const std::wstring& tokenPathOverride = L"");
    };
}
