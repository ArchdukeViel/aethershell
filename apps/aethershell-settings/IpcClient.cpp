#include "pch.h"
#include "IpcClient.h"
#include <sstream>

namespace AetherShell::Settings
{
    std::wstring SettingsIpcClient::ResolvePipeName()
    {
        DWORD sessionId = 1;
        ProcessIdToSessionId(GetCurrentProcessId(), &sessionId);
        std::wstringstream ss;
        ss << L"\\\\.\\pipe\\aethershell-ipc-" << sessionId;
        return ss.str();
    }

    IpcResult SettingsIpcClient::SendCommand(const std::string& command, const std::string& jsonPayload)
    {
        IpcResult result;
        std::wstring pipePath = ResolvePipeName();

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
            result.success = false;
            result.error = "AetherShell Core is not running or named pipe inaccessible.";
            return result;
        }

        // Build framed IPC JSON message: {"protocol_version":1,"message_id":"...","command":"...","payload":...}
        std::stringstream req;
        req << "{\"protocol_version\":1,\"message_id\":\"settings-cmd\",\"command\":\""
            << command << "\",\"payload\":" << jsonPayload << ",\"deadline_ms\":5000}";
        std::string reqBody = req.str();

        uint32_t lengthPrefix = static_cast<uint32_t>(reqBody.size());
        DWORD written = 0;

        // Write 4-byte length prefix + payload
        if (!WriteFile(hPipe, &lengthPrefix, sizeof(lengthPrefix), &written, nullptr) ||
            !WriteFile(hPipe, reqBody.data(), lengthPrefix, &written, nullptr))
        {
            CloseHandle(hPipe);
            result.success = false;
            result.error = "Failed writing request to IPC pipe.";
            return result;
        }

        // Read 4-byte response length
        uint32_t respLength = 0;
        DWORD bytesRead = 0;
        if (!ReadFile(hPipe, &respLength, sizeof(respLength), &bytesRead, nullptr) || bytesRead != sizeof(respLength))
        {
            CloseHandle(hPipe);
            result.success = false;
            result.error = "Failed reading response length from IPC pipe.";
            return result;
        }

        if (respLength > 1024 * 1024) // 1 MiB safety ceiling
        {
            CloseHandle(hPipe);
            result.success = false;
            result.error = "Response length exceeded 1 MiB framing ceiling.";
            return result;
        }

        std::vector<char> buffer(respLength);
        if (!ReadFile(hPipe, buffer.data(), respLength, &bytesRead, nullptr) || bytesRead != respLength)
        {
            CloseHandle(hPipe);
            result.success = false;
            result.error = "Failed reading response body from IPC pipe.";
            return result;
        }

        CloseHandle(hPipe);
        result.success = true;
        result.payload = std::string(buffer.data(), buffer.size());
        return result;
    }
}
