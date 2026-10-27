#include "pch.h"
#include "App.xaml.h"
#include "MainWindow.xaml.h"
#include "IpcClient.h"
#include <shellapi.h>
#if __has_include("XamlTypeInfo.Impl.g.cpp")
#include "XamlTypeInfo.Impl.g.cpp"
#endif
#if __has_include("XamlTypeInfo.g.cpp")
#include "XamlTypeInfo.g.cpp"
#endif
#if __has_include("XamlMetaDataProvider.g.cpp")
#include "XamlMetaDataProvider.g.cpp"
#endif
#if __has_include("module.g.cpp")
#include "module.g.cpp"
#endif

using namespace winrt;
using namespace Microsoft::UI::Xaml;

namespace winrt::AetherShell::Settings::implementation
{
    App::App()
    {
        InitializeComponent();
    }

    void App::OnLaunched(Microsoft::UI::Xaml::LaunchActivatedEventArgs const&)
    {
        m_window = make<MainWindow>();
        m_window.Activate();
    }
}

int __stdcall wWinMain(HINSTANCE, HINSTANCE, PWSTR, int)
{
    // Narrow compatibility smoke test handler (PR #20 directive F5)
    int argc = 0;
    LPWSTR* argv = CommandLineToArgvW(GetCommandLineW(), &argc);
    if (argv)
    {
        std::wstring testPipe;
        std::wstring testTokenPath;
        std::string testCmd;
        std::string testPayload = "{}";
        bool isTest = false;

        for (int i = 1; i < argc; ++i)
        {
            if (wcscmp(argv[i], L"--test-ipc-pipe") == 0 && i + 1 < argc)
            {
                testPipe = argv[i + 1];
                ++i;
            }
            else if (wcscmp(argv[i], L"--test-ipc-token") == 0 && i + 1 < argc)
            {
                testTokenPath = argv[i + 1];
                ++i;
            }
            else if (wcscmp(argv[i], L"--test-ipc-command") == 0 && i + 1 < argc)
            {
                isTest = true;
                testCmd = winrt::to_string(argv[i + 1]);
                ++i;
                if (i + 1 < argc && argv[i + 1][0] != L'-')
                {
                    testPayload = winrt::to_string(argv[i + 1]);
                    ++i;
                }
            }
        }

        if (isTest)
        {
            auto res = ::AetherShell::Settings::SettingsIpcClient::SendCommand(
                testCmd, testPayload, testPipe, testTokenPath);

            // Write directly to standard output handle for test capture
            HANDLE hStdout = GetStdHandle(STD_OUTPUT_HANDLE);
            std::string out = res.success
                ? ("IPC_TEST_SUCCESS:" + res.payload + "\n")
                : ("IPC_TEST_ERROR:" + res.error + "\n");
            DWORD written = 0;
            if (hStdout != INVALID_HANDLE_VALUE && hStdout != nullptr)
            {
                WriteFile(hStdout, out.data(), static_cast<DWORD>(out.size()), &written, nullptr);
            }

            LocalFree(argv);
            return res.success ? 0 : 1;
        }
        LocalFree(argv);
    }

    winrt::init_apartment();
    ::winrt::Microsoft::UI::Xaml::Application::Start(
        [](auto&&)
        {
            ::winrt::make<::winrt::AetherShell::Settings::implementation::App>();
        });

    return 0;
}
