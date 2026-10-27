#include "pch.h"
#include "IpcClient.h"
#include <winrt/Windows.Data.Json.h>
#include <fstream>
#include <sstream>

namespace AetherShell::Settings
{
    std::wstring SettingsIpcClient::ResolvePipeName(const std::wstring& pipeOverride)
    {
        // Explicit pipe override is only used when injected (e.g. via --test-ipc-pipe CLI flag).
        // Ordinary production execution never passes an override and will always connect
        // to the authoritative session-scoped pipe, preventing accidental test endpoint leakage.
        if (!pipeOverride.empty())
        {
            return pipeOverride;
        }

        DWORD sessionId = 1;
        ProcessIdToSessionId(GetCurrentProcessId(), &sessionId);
        std::wstringstream ss;
        ss << L"\\\\.\\pipe\\aethershell-ipc-" << sessionId;
        return ss.str();
    }

    std::string SettingsIpcClient::LoadSessionToken(const std::wstring& tokenPathOverride)
    {
        // 1. Explicit override path (e.g. from --test-ipc-token CLI argument)
        if (!tokenPathOverride.empty())
        {
            std::ifstream file(tokenPathOverride);
            if (file.is_open())
            {
                std::string tok;
                if (std::getline(file, tok))
                {
                    while (!tok.empty() && (tok.back() == '\r' || tok.back() == '\n' || tok.back() == ' ')) tok.pop_back();
                    if (!tok.empty()) return tok;
                }
            }
        }

        // 2. Check override environment variable AETHERSHELL_SESSION_TOKEN_PATH
        wchar_t overridePath[MAX_PATH] = { 0 };
        DWORD len = GetEnvironmentVariableW(L"AETHERSHELL_SESSION_TOKEN_PATH", overridePath, MAX_PATH);
        if (len > 0 && len < MAX_PATH)
        {
            std::ifstream file(overridePath);
            if (file.is_open())
            {
                std::string tok;
                if (std::getline(file, tok))
                {
                    while (!tok.empty() && (tok.back() == '\r' || tok.back() == '\n' || tok.back() == ' ')) tok.pop_back();
                    if (!tok.empty()) return tok;
                }
            }
        }

        // 3. Resolve per-session token path: %LOCALAPPDATA%\AetherShell\session_{sessionId}.token
        DWORD sessionId = 1;
        ProcessIdToSessionId(GetCurrentProcessId(), &sessionId);

        wchar_t localAppData[MAX_PATH] = { 0 };
        len = GetEnvironmentVariableW(L"LOCALAPPDATA", localAppData, MAX_PATH);
        if (len > 0 && len < MAX_PATH)
        {
            std::wstring sessionPath = std::wstring(localAppData) + L"\\AetherShell\\session_" + std::to_wstring(sessionId) + L".token";
            std::ifstream file(sessionPath);
            if (file.is_open())
            {
                std::string tok;
                if (std::getline(file, tok))
                {
                    while (!tok.empty() && (tok.back() == '\r' || tok.back() == '\n' || tok.back() == ' ')) tok.pop_back();
                    if (!tok.empty()) return tok;
                }
            }

            // Fallback to legacy session.token path
            std::wstring fallbackPath = std::wstring(localAppData) + L"\\AetherShell\\session.token";
            std::ifstream fallbackFile(fallbackPath);
            if (fallbackFile.is_open())
            {
                std::string tok;
                if (std::getline(fallbackFile, tok))
                {
                    while (!tok.empty() && (tok.back() == '\r' || tok.back() == '\n' || tok.back() == ' ')) tok.pop_back();
                    if (!tok.empty()) return tok;
                }
            }
        }

        return "";
    }

    IpcResult SettingsIpcClient::SendCommand(
        const std::string& command,
        const std::string& jsonPayload,
        const std::wstring& pipeOverride,
        const std::wstring& tokenPathOverride)
    {
        IpcResult result;
        std::wstring pipePath = ResolvePipeName(pipeOverride);

        HANDLE hPipe = CreateFileW(
            pipePath.c_str(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            nullptr,
            OPEN_EXISTING,
            0,
            nullptr
        );

        if (hPipe == INVALID_HANDLE_VALUE)
        {
            result.transportSuccess = false;
            result.success = false;
            result.error = "AetherShell Core is not running or named pipe inaccessible.";
            return result;
        }

        // Attach session capability token to payload for privileged management commands
        std::string effectivePayload = jsonPayload;
        std::string token = LoadSessionToken(tokenPathOverride);
        if (!token.empty() && effectivePayload.find("session_token") == std::string::npos)
        {
            if (effectivePayload.empty() || effectivePayload == "{}")
            {
                effectivePayload = "{\"session_token\":\"" + token + "\"}";
            }
            else
            {
                size_t openBrace = effectivePayload.find('{');
                if (openBrace != std::string::npos)
                {
                    effectivePayload.insert(openBrace + 1, "\"session_token\":\"" + token + "\",");
                }
            }
        }

        // Build framed IPC JSON message: {"protocol_version":1,"message_id":"...","command":"...","payload":...}
        std::stringstream req;
        req << "{\"protocol_version\":1,\"message_id\":\"settings-cmd\",\"command\":\""
            << command << "\",\"payload\":" << effectivePayload << ",\"deadline_ms\":5000}";
        std::string reqBody = req.str();

        uint32_t lengthPrefix = static_cast<uint32_t>(reqBody.size());
        DWORD written = 0;
        DWORD totalWritten = 0;

        // Write 4-byte length prefix in a robust loop
        while (totalWritten < sizeof(lengthPrefix))
        {
            if (!WriteFile(hPipe, reinterpret_cast<const char*>(&lengthPrefix) + totalWritten, sizeof(lengthPrefix) - totalWritten, &written, nullptr) || written == 0)
            {
                CloseHandle(hPipe);
                result.transportSuccess = false;
                result.success = false;
                result.error = "Failed writing request length prefix to IPC pipe.";
                return result;
            }
            totalWritten += written;
        }

        // Write request payload in a robust loop
        totalWritten = 0;
        while (totalWritten < lengthPrefix)
        {
            if (!WriteFile(hPipe, reqBody.data() + totalWritten, lengthPrefix - totalWritten, &written, nullptr) || written == 0)
            {
                CloseHandle(hPipe);
                result.transportSuccess = false;
                result.success = false;
                result.error = "Failed writing request body to IPC pipe.";
                return result;
            }
            totalWritten += written;
        }

        // Read 4-byte response length in a robust loop
        uint32_t respLength = 0;
        DWORD totalRead = 0;
        while (totalRead < sizeof(respLength))
        {
            DWORD bytesRead = 0;
            if (!ReadFile(hPipe, reinterpret_cast<char*>(&respLength) + totalRead, sizeof(respLength) - totalRead, &bytesRead, nullptr) || bytesRead == 0)
            {
                CloseHandle(hPipe);
                result.transportSuccess = false;
                result.success = false;
                result.error = "Failed reading response length from IPC pipe.";
                return result;
            }
            totalRead += bytesRead;
        }

        if (respLength == 0)
        {
            CloseHandle(hPipe);
            result.transportSuccess = true;
            result.success = false;
            result.error = "Malformed IPC response: zero-length response payload.";
            return result;
        }

        if (respLength > 1024 * 1024) // 1 MiB safety ceiling
        {
            CloseHandle(hPipe);
            result.transportSuccess = true;
            result.success = false;
            result.error = "Response length exceeded 1 MiB framing ceiling.";
            return result;
        }

        std::vector<char> buffer(respLength);
        totalRead = 0;
        while (totalRead < respLength)
        {
            DWORD bytesRead = 0;
            if (!ReadFile(hPipe, buffer.data() + totalRead, respLength - totalRead, &bytesRead, nullptr) || bytesRead == 0)
            {
                CloseHandle(hPipe);
                result.transportSuccess = false;
                result.success = false;
                result.error = "Failed reading response body from IPC pipe.";
                return result;
            }
            totalRead += bytesRead;
        }

        CloseHandle(hPipe);
        result.transportSuccess = true;

        std::string rawJson(buffer.data(), buffer.size());
        winrt::Windows::Data::Json::JsonObject root;
        if (!winrt::Windows::Data::Json::JsonObject::TryParse(winrt::to_hstring(rawJson), root))
        {
            result.success = false;
            result.error = "Malformed IPC response: invalid JSON syntax.";
            return result;
        }

        if (!root.HasKey(L"protocol_version"))
        {
            result.success = false;
            result.error = "Malformed IPC response: missing protocol_version.";
            return result;
        }

        auto protoVal = root.GetNamedValue(L"protocol_version");
        if (protoVal.ValueType() != winrt::Windows::Data::Json::JsonValueType::Number ||
            static_cast<uint32_t>(protoVal.GetNumber()) != 1)
        {
            result.success = false;
            result.error = "Malformed IPC response: unsupported protocol_version.";
            return result;
        }

        if (!root.HasKey(L"success"))
        {
            result.success = false;
            result.error = "Malformed IPC response: missing success field.";
            return result;
        }

        auto successVal = root.GetNamedValue(L"success");
        if (successVal.ValueType() != winrt::Windows::Data::Json::JsonValueType::Boolean)
        {
            result.success = false;
            result.error = "Malformed IPC response: success field is not boolean.";
            return result;
        }

        bool serverSuccess = successVal.GetBoolean();
        if (serverSuccess)
        {
            result.success = true;
            if (root.HasKey(L"payload"))
            {
                auto payloadVal = root.GetNamedValue(L"payload");
                result.payload = winrt::to_string(payloadVal.Stringify());
            }
            else
            {
                result.payload = "{}";
            }
        }
        else
        {
            result.success = false;
            if (root.HasKey(L"error_message"))
            {
                auto errVal = root.GetNamedValue(L"error_message");
                if (errVal.ValueType() == winrt::Windows::Data::Json::JsonValueType::String)
                {
                    result.error = winrt::to_string(errVal.GetString());
                }
                else
                {
                    result.error = "Server rejected command with non-string error_message.";
                }
            }
            else
            {
                result.error = "Server rejected command without error_message.";
            }
        }

        return result;
    }
}
