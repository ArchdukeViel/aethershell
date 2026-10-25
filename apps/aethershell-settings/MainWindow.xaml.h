#pragma once
#include "MainWindow.xaml.g.h"

namespace winrt::AetherShell::Settings::implementation
{
    struct MainWindow : MainWindowT<MainWindow>
    {
        MainWindow();
        void NavView_SelectionChanged(
            winrt::Microsoft::UI::Xaml::Controls::NavigationView const& sender,
            winrt::Microsoft::UI::Xaml::Controls::NavigationViewSelectionChangedEventArgs const& args);

        void OnWindowClosed(
            winrt::Windows::Foundation::IInspectable const& sender,
            winrt::Microsoft::UI::Xaml::WindowEventArgs const& args);
    };
}

namespace winrt::AetherShell::Settings::factory_implementation
{
    struct MainWindow : MainWindowT<MainWindow, implementation::MainWindow>
    {
    };
}
