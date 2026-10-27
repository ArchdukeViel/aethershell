-- AetherShell Bundled Trusted Script (PRD Section 13.2 / Q32:B / Gate G10)
-- Demonstrates unrestricted trusted Lua 5.4 host integration,
-- querying telemetry snapshots and publishing custom bindings.

local counter = 0

function init()
    aether.set_binding("script.status", "Lua 5.4 Active")
    aether.set_binding("script.custom_summary", "Healthy - Initialized")
end

function update()
    counter = counter + 1
    if counter % 10 == 0 then
        local telem = aether.get_telemetry()
        local cpu = 0.0
        if telem and telem["cpu.usage_percent"] then
            cpu = telem["cpu.usage_percent"]
        end
        aether.publish_bindings({
            ["script.status"] = "Lua 5.4 Active",
            ["script.tick_count"] = counter,
            ["script.custom_summary"] = string.format("CPU: %.1f%% | Tick: %d", cpu, counter)
        })
    end
end
