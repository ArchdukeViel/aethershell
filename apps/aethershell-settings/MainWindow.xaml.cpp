#include "pch.h"
#include "MainWindow.xaml.h"
#if __has_include("MainWindow.g.cpp")
#include "MainWindow.g.cpp"
#endif
#include "IpcClient.h"

using namespace winrt;
using namespace Microsoft::UI::Xaml;
using namespace Microsoft::UI::Xaml::Controls;
using namespace Microsoft::UI::Xaml::Media;

namespace winrt::AetherShell::Settings::implementation
{
    MainWindow::MainWindow()
    {
        InitializeComponent();
        this->Title(L"AetherShell Settings & Diagnostics");
        this->Closed({ this, &MainWindow::OnWindowClosed });

        // Default navigation to General page on launch
        NavigateToPage(L"general");
    }

    void MainWindow::NavView_SelectionChanged(
        NavigationView const&,
        NavigationViewSelectionChangedEventArgs const& args)
    {
        if (auto selectedItem = args.SelectedItemContainer())
        {
            auto tag = unbox_value<hstring>(selectedItem.Tag());
            NavigateToPage(tag);
        }
    }

    void MainWindow::NavigateToPage(hstring const& tag)
    {
        if (!ContentFrame())
        {
            return;
        }

        if (tag == L"general")
        {
            ContentFrame().Content(CreateGeneralView());
        }
        else if (tag == L"taskbar")
        {
            ContentFrame().Content(CreateTaskbarView());
        }
        else if (tag == L"desktop")
        {
            ContentFrame().Content(CreateDesktopView());
        }
        else if (tag == L"widgets")
        {
            ContentFrame().Content(CreateWidgetsView());
        }
        else if (tag == L"audio")
        {
            ContentFrame().Content(CreateAudioView());
        }
        else if (tag == L"scripts")
        {
            ContentFrame().Content(CreateScriptsView());
        }
    }

    void MainWindow::OnWindowClosed(
        Windows::Foundation::IInspectable const&,
        WindowEventArgs const&)
    {
        // Enforce Gate G11: Process exits cleanly on close to eliminate resident RAM footprint (0 MB resident)
        if (auto app = Application::Current())
        {
            app.Exit();
        }
        else
        {
            PostQuitMessage(0);
        }
    }

    UIElement MainWindow::CreateGeneralView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"General Configuration");
        title.FontSize(24);
        panel.Children().Append(title);

        // Query Daemon status over IPC
        auto ipcStatus = ::AetherShell::Settings::SettingsIpcClient::SendCommand("get_status");
        TextBlock statusText;
        if (ipcStatus.success)
        {
            statusText.Text(L"Core Engine: Active & Connected (Named Pipe IPC)");
        }
        else
        {
            statusText.Text(L"Core Engine: Disconnected (Core not currently resident)");
        }
        panel.Children().Append(statusText);

        ToggleSwitch autostartSwitch;
        autostartSwitch.Header(box_value(L"Start AetherShell automatically with Windows"));
        autostartSwitch.IsOn(true);
        panel.Children().Append(autostartSwitch);

        ComboBox langBox;
        langBox.Header(box_value(L"Display Language"));
        ComboBoxItem itemEn, itemJa, itemDe;
        itemEn.Content(box_value(L"English (United States)"));
        itemJa.Content(box_value(L"Japanese (日本語)"));
        itemDe.Content(box_value(L"German (Deutsch)"));
        langBox.Items().Append(itemEn);
        langBox.Items().Append(itemJa);
        langBox.Items().Append(itemDe);
        langBox.SelectedIndex(0);
        panel.Children().Append(langBox);

        ComboBox logBox;
        logBox.Header(box_value(L"Diagnostics Logging Level"));
        ComboBoxItem logInfo, logDebug, logTrace;
        logInfo.Content(box_value(L"Information (Recommended)"));
        logDebug.Content(box_value(L"Debug (Verbose)"));
        logTrace.Content(box_value(L"Trace (Full Diagnostic)"));
        logBox.Items().Append(logInfo);
        logBox.Items().Append(logDebug);
        logBox.Items().Append(logTrace);
        logBox.SelectedIndex(0);
        panel.Children().Append(logBox);

        Button shutdownBtn;
        shutdownBtn.Content(box_value(L"Terminate Core Daemon"));
        shutdownBtn.Click([](auto const&, auto const&) {
            ::AetherShell::Settings::SettingsIpcClient::SendCommand("shutdown");
        });
        panel.Children().Append(shutdownBtn);

        return panel;
    }

    UIElement MainWindow::CreateTaskbarView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"Taskbar & State Management");
        title.FontSize(24);
        panel.Children().Append(title);

        ToggleSwitch multiMonSwitch;
        multiMonSwitch.Header(box_value(L"Render Taskbars on Secondary Displays"));
        multiMonSwitch.IsOn(true);
        panel.Children().Append(multiMonSwitch);

        ToggleSwitch autoHideSwitch;
        autoHideSwitch.Header(box_value(L"Auto-hide on Fullscreen Applications"));
        autoHideSwitch.IsOn(true);
        panel.Children().Append(autoHideSwitch);

        ComboBox alignBox;
        alignBox.Header(box_value(L"Taskbar Alignment"));
        ComboBoxItem leftAlign, centerAlign;
        leftAlign.Content(box_value(L"Left Aligned"));
        centerAlign.Content(box_value(L"Center Aligned (Default)"));
        alignBox.Items().Append(leftAlign);
        alignBox.Items().Append(centerAlign);
        alignBox.SelectedIndex(1);
        panel.Children().Append(alignBox);

        Slider blurSlider;
        blurSlider.Header(box_value(L"Acrylic / Mica Accent Blur Opacity"));
        blurSlider.Minimum(0);
        blurSlider.Maximum(100);
        blurSlider.Value(85);
        panel.Children().Append(blurSlider);

        return panel;
    }

    UIElement MainWindow::CreateDesktopView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"Desktop Layering & Interactive Mode");
        title.FontSize(24);
        panel.Children().Append(title);

        TextBlock desc;
        desc.Text(L"Per PRD Q31:A: Desktop widgets are layered strictly below desktop icons and above wallpapers. Temporary Interactive Mode brings widgets into focus for manipulation without desktop icon obstruction.");
        desc.TextWrapping(TextWrapping::Wrap);
        panel.Children().Append(desc);

        Slider timeoutSlider;
        timeoutSlider.Header(box_value(L"Interactive Inactivity Timeout (Seconds)"));
        timeoutSlider.Minimum(2);
        timeoutSlider.Maximum(30);
        timeoutSlider.Value(8);
        panel.Children().Append(timeoutSlider);

        Button triggerBtn;
        triggerBtn.Content(box_value(L"Enter Temporary Interactive Mode (8s)"));
        triggerBtn.Click([](auto const&, auto const&) {
            ::AetherShell::Settings::SettingsIpcClient::SendCommand("enter_interactive_mode", "{\"timeout_seconds\":8}");
        });
        panel.Children().Append(triggerBtn);

        return panel;
    }

    UIElement MainWindow::CreateWidgetsView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"Widgets & Visual Themes");
        title.FontSize(24);
        panel.Children().Append(title);

        ComboBox themeBox;
        themeBox.Header(box_value(L"Active Theme"));
        const wchar_t* themes[] = { L"obsidian", L"neon_grid", L"monochrome", L"arctic", L"aurora", L"solar" };
        for (const auto& theme : themes)
        {
            ComboBoxItem item;
            item.Content(box_value(theme));
            themeBox.Items().Append(item);
        }
        themeBox.SelectedIndex(0);
        panel.Children().Append(themeBox);

        ComboBox profileBox;
        profileBox.Header(box_value(L"Profile Archetype"));
        const wchar_t* archetypes[] = { L"signature_minimal", L"cyber_hud", L"glass_bento", L"ambient_soft", L"terminal_mono", L"spectrum_dynamic" };
        for (const auto& arch : archetypes)
        {
            ComboBoxItem item;
            item.Content(box_value(arch));
            profileBox.Items().Append(item);
        }
        profileBox.SelectedIndex(0);
        panel.Children().Append(profileBox);

        TextBlock info;
        info.Text(L"Enabled Built-in Widgets: System Clock, CPU/GPU Telemetry, WASAPI Spectrum Visualizer, Network Traffic Monitor.");
        panel.Children().Append(info);

        return panel;
    }

    UIElement MainWindow::CreateAudioView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"WASAPI Audio Visualization (PRD Section 10)");
        title.FontSize(24);
        panel.Children().Append(title);

        TextBlock privacy;
        privacy.Text(L"Strict Audio Privacy Guarantee (Gate G14): Only frequency magnitudes are processed for UI visualization. Raw PCM audio is never recorded, buffered to disk, or transmitted over network.");
        privacy.TextWrapping(TextWrapping::Wrap);
        panel.Children().Append(privacy);

        ToggleSwitch audioSwitch;
        audioSwitch.Header(box_value(L"WASAPI Loopback Capture (Default: Enabled)"));
        audioSwitch.IsOn(true);
        audioSwitch.Toggled([](IInspectable const& sender, RoutedEventArgs const&) {
            auto ts = sender.as<ToggleSwitch>();
            if (ts.IsOn())
            {
                ::AetherShell::Settings::SettingsIpcClient::SendCommand("resume_audio");
            }
            else
            {
                ::AetherShell::Settings::SettingsIpcClient::SendCommand("pause_audio");
            }
        });
        panel.Children().Append(audioSwitch);

        Button querySpectrumBtn;
        querySpectrumBtn.Content(box_value(L"Query Active Audio Spectrum"));
        querySpectrumBtn.Click([](auto const&, auto const&) {
            ::AetherShell::Settings::SettingsIpcClient::SendCommand("get_spectrum");
        });
        panel.Children().Append(querySpectrumBtn);

        return panel;
    }

    UIElement MainWindow::CreateScriptsView()
    {
        StackPanel panel;
        panel.Spacing(16);

        TextBlock title;
        title.Text(L"Script Host & Security Isolation (PRD Section 11 / Q33:A)");
        title.FontSize(24);
        panel.Children().Append(title);

        TextBlock desc;
        desc.Text(L"Lua scripts execute inside an isolated supervisor process (aethershell-script-host.exe). Memory, file access, and execution time are strictly bounded.");
        desc.TextWrapping(TextWrapping::Wrap);
        panel.Children().Append(desc);

        ToggleSwitch scriptSwitch;
        scriptSwitch.Header(box_value(L"Enable Lua Script Execution"));
        scriptSwitch.IsOn(true);
        panel.Children().Append(scriptSwitch);

        ComboBox trustBox;
        trustBox.Header(box_value(L"Script Execution Trust Level"));
        ComboBoxItem trustSandbox, trustPrompt, trustFull;
        trustSandbox.Content(box_value(L"Restricted Sandbox (No OS/Filesystem Access)"));
        trustPrompt.Content(box_value(L"Prompted Execution (Explicit Consent Required)"));
        trustFull.Content(box_value(L"Full Machine Trust (Local scripts only)"));
        trustBox.Items().Append(trustSandbox);
        trustBox.Items().Append(trustPrompt);
        trustBox.Items().Append(trustFull);
        trustBox.SelectedIndex(0);
        panel.Children().Append(trustBox);

        return panel;
    }
}
