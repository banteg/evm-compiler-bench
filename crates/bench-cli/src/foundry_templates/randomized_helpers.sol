    function _next(uint256 state) internal pure returns (uint256) {
        return uint256(keccak256(abi.encodePacked(state)));
    }

    function _appendTrace(string memory traceLog, string memory trace) internal pure returns (string memory) {
        if (bytes(traceLog).length == 0) {
            return trace;
        }
        return string.concat(traceLog, ";", trace);
    }

    function _actor(uint256 value) internal view returns (address) {
        uint256 index = value % 3;
        if (index == 0) return address(this);
        if (index == 1) return BOB;
        return CAROL;
    }

    function _actorName(address actor) internal view returns (string memory) {
        if (actor == address(this)) return "this";
        if (actor == BOB) return "BOB";
        if (actor == CAROL) return "CAROL";
        return "unknown";
    }

    function _writeFailure(
        string memory kind,
        string memory benchmarkId,
        uint256 seed,
        uint256 step,
        string memory traceLog,
        string memory detail
    ) internal {
        vm.createDir("../results/raw/failures", true);
        vm.writeFile(
            string.concat(
                "../results/raw/failures/",
                benchmarkId,
                "-",
                kind,
                "-",
                vm.toString(seed),
                "-",
                vm.toString(step),
                ".json"
            ),
            string.concat(
                "{\"kind\":\"", kind,
                "\",\"benchmark_id\":\"", benchmarkId,
                "\",\"seed\":", vm.toString(seed),
                ",\"step\":", vm.toString(step),
                ",\"trace\":\"", traceLog,
                "\",\"detail\":\"", detail,
                "\"}"
            )
        );
    }

    function _requireCheck(
        bool condition,
        string memory kind,
        string memory benchmarkId,
        uint256 seed,
        uint256 step,
        string memory traceLog,
        string memory detail
    ) internal {
        if (!condition) {
            _writeFailure(kind, benchmarkId, seed, step, traceLog, detail);
            require(condition, detail);
        }
    }

    function _runBoth(
        address solTarget,
        address vyperTarget,
        bytes memory data,
        uint256 value,
        address sender,
        string memory benchmarkId,
        uint256 seed,
        uint256 step,
        string memory traceLog
    ) internal {
        (bool solOk, bytes32 solHash,) = _run(solTarget, data, value, sender);
        (bool vyperOk, bytes32 vyperHash,) = _run(vyperTarget, data, value, sender);
        _requireCheck(solOk == vyperOk, "randomized_differential", benchmarkId, seed, step, traceLog, "status mismatch");
        if (solOk) {
            _requireCheck(solHash == vyperHash, "randomized_differential", benchmarkId, seed, step, traceLog, "return mismatch");
        }
    }

    function _compareState(
        bytes32 solState,
        bytes32 vyperState,
        string memory kind,
        string memory benchmarkId,
        uint256 seed,
        uint256 step,
        string memory traceLog
    ) internal {
        _requireCheck(solState == vyperState, kind, benchmarkId, seed, step, traceLog, "state mismatch");
    }

    function _readUint(address target, bytes memory data) internal returns (uint256 value) {
        (bool ok, bytes memory ret) = target.call(data);
        require(ok, "uint read failed");
        value = abi.decode(ret, (uint256));
    }

    function _readBool(address target, bytes memory data) internal returns (bool value) {
        (bool ok, bytes memory ret) = target.call(data);
        require(ok, "bool read failed");
        value = abi.decode(ret, (bool));
    }

    function _readAddress(address target, bytes memory data) internal returns (address value) {
        (bool ok, bytes memory ret) = target.call(data);
        require(ok, "address read failed");
        value = abi.decode(ret, (address));
    }

    function _readReserves(address target) internal returns (uint256 reserve0, uint256 reserve1) {
        (bool ok, bytes memory ret) = target.call(abi.encodeWithSignature("getReserves()"));
        require(ok, "reserve read failed");
        (reserve0, reserve1) = abi.decode(ret, (uint256, uint256));
    }

    function _counterState(address target) internal returns (bytes32) {
        return keccak256(abi.encode(_readUint(target, abi.encodeWithSignature("value()"))));
    }

    function _erc20State(address target) internal returns (bytes32) {
        return keccak256(abi.encode(
            _readUint(target, abi.encodeWithSignature("totalSupply()")),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", address(this))),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", BOB)),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", CAROL)),
            _readUint(target, abi.encodeWithSignature("allowance(address,address)", address(this), BOB)),
            _readUint(target, abi.encodeWithSignature("allowance(address,address)", BOB, CAROL))
        ));
    }

    function _vaultState(address target) internal returns (bytes32) {
        return keccak256(abi.encode(
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", address(this))),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", BOB)),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", CAROL)),
            _readUint(target, abi.encodeWithSignature("totalShares()")),
            _readUint(target, abi.encodeWithSignature("totalAssets()"))
        ));
    }

    function _ownableState(address target) internal returns (bytes32) {
        return keccak256(abi.encode(
            _readAddress(target, abi.encodeWithSignature("owner()")),
            _readBool(target, abi.encodeWithSignature("paused()")),
            _readUint(target, abi.encodeWithSignature("counter()"))
        ));
    }

    function _ammState(address target) internal returns (bytes32) {
        (uint256 reserve0FromPair, uint256 reserve1FromPair) = _readReserves(target);
        return keccak256(abi.encode(
            _readUint(target, abi.encodeWithSignature("reserve0()")),
            _readUint(target, abi.encodeWithSignature("reserve1()")),
            _readUint(target, abi.encodeWithSignature("totalLiquidity()")),
            reserve0FromPair,
            reserve1FromPair
        ));
    }

    struct OwnableModel {
        address owner;
        bool paused;
        uint256 counter;
        string traceLog;
    }

    function _requireOwnableProperty(bool condition, uint256 seed, uint256 step, string memory traceLog, string memory detail) internal {
        _requireCheck(condition, "property", "ownable_pausable", seed, step, traceLog, detail);
    }

    function _randomDiff_counter(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 4;
            string memory trace;
            if (op == 0) {
                trace = "increment";
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("increment()"), 0, address(this), "counter", seed, i, traceLog);
            } else if (op == 1) {
                uint256 amount = rng % 17;
                trace = string.concat("add_value:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("add_value(uint256)", amount), 0, address(this), "counter", seed, i, traceLog);
            } else if (op == 2) {
                trace = "reset";
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("reset()"), 0, address(this), "counter", seed, i, traceLog);
            } else {
                trace = "value";
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("value()"), 0, address(this), "counter", seed, i, traceLog);
            }
            _compareState(_counterState(solTarget), _counterState(vyperTarget), "randomized_differential", "counter", seed, i, traceLog);
        }
    }

    function _randomDiff_erc20_minimal(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 5;
            uint256 amount = ((rng % 20) + 1) * 1 ether;
            string memory trace;
            if (op == 0) {
                trace = string.concat("transfer_this_to_BOB:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("transfer(address,uint256)", BOB, amount), 0, address(this), "erc20_minimal", seed, i, traceLog);
            } else if (op == 1) {
                trace = string.concat("transfer_BOB_to_CAROL:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("transfer(address,uint256)", CAROL, amount), 0, BOB, "erc20_minimal", seed, i, traceLog);
            } else if (op == 2) {
                trace = string.concat("approve_this_to_BOB:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("approve(address,uint256)", BOB, amount), 0, address(this), "erc20_minimal", seed, i, traceLog);
            } else if (op == 3) {
                trace = string.concat("transferFrom_this_to_CAROL_by_BOB:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("transferFrom(address,address,uint256)", address(this), CAROL, amount), 0, BOB, "erc20_minimal", seed, i, traceLog);
            } else {
                trace = string.concat("approve_BOB_to_CAROL:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("approve(address,uint256)", CAROL, amount), 0, BOB, "erc20_minimal", seed, i, traceLog);
            }
            _compareState(_erc20State(solTarget), _erc20State(vyperTarget), "randomized_differential", "erc20_minimal", seed, i, traceLog);
        }
    }

    function _randomDiff_vault_deposit_withdraw(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            address actor = _actor(rng);
            uint256 amount = ((rng % 9) + 1) * (1 ether / 10);
            string memory trace;
            if (rng % 2 == 0) {
                trace = string.concat("deposit_", _actorName(actor), ":", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("deposit()"), amount, actor, "vault_deposit_withdraw", seed, i, traceLog);
            } else {
                trace = string.concat("withdraw_", _actorName(actor), ":", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("withdraw(uint256)", amount), 0, actor, "vault_deposit_withdraw", seed, i, traceLog);
            }
            _compareState(_vaultState(solTarget), _vaultState(vyperTarget), "randomized_differential", "vault_deposit_withdraw", seed, i, traceLog);
        }
    }

    function _randomDiff_ownable_pausable(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 4;
            address actor = _actor(rng);
            string memory trace;
            if (op == 0) {
                trace = string.concat("pause_", _actorName(actor));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("pause()"), 0, actor, "ownable_pausable", seed, i, traceLog);
            } else if (op == 1) {
                trace = string.concat("unpause_", _actorName(actor));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("unpause()"), 0, actor, "ownable_pausable", seed, i, traceLog);
            } else if (op == 2) {
                trace = string.concat("guardedIncrement_", _actorName(actor));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("guardedIncrement()"), 0, actor, "ownable_pausable", seed, i, traceLog);
            } else {
                address newOwner = _actor(rng / 7);
                trace = string.concat("transferOwnership_", _actorName(actor), "_to_", _actorName(newOwner));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("transferOwnership(address)", newOwner), 0, actor, "ownable_pausable", seed, i, traceLog);
            }
            _compareState(_ownableState(solTarget), _ownableState(vyperTarget), "randomized_differential", "ownable_pausable", seed, i, traceLog);
        }
    }

    function _randomDiff_amm_pair_subset(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 4;
            if (op == 0) {
                traceLog = _randomDiffAmmMint(solTarget, vyperTarget, seed, i, rng, traceLog);
            } else if (op == 1) {
                traceLog = _randomDiffAmmBurn(solTarget, vyperTarget, seed, i, rng, traceLog);
            } else if (op == 2) {
                traceLog = _randomDiffAmmSwap(solTarget, vyperTarget, seed, i, rng, traceLog);
            } else {
                traceLog = _randomDiffAmmSync(solTarget, vyperTarget, seed, i, rng, traceLog);
            }
            _compareState(_ammState(solTarget), _ammState(vyperTarget), "randomized_differential", "amm_pair_subset", seed, i, traceLog);
        }
    }

    function _randomDiffAmmMint(
        address solTarget,
        address vyperTarget,
        uint256 seed,
        uint256 step,
        uint256 rng,
        string memory traceLog
    ) internal returns (string memory) {
        uint256 amount0 = (rng % 200) + 1;
        uint256 amount1 = ((rng / 17) % 200) + 1;
        traceLog = _appendTrace(traceLog, string.concat("mint:", vm.toString(amount0), ":", vm.toString(amount1)));
        _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("mint(uint256,uint256)", amount0, amount1), 0, address(this), "amm_pair_subset", seed, step, traceLog);
        return traceLog;
    }

    function _randomDiffAmmBurn(
        address solTarget,
        address vyperTarget,
        uint256 seed,
        uint256 step,
        uint256 rng,
        string memory traceLog
    ) internal returns (string memory) {
        uint256 liquidity = _readUint(solTarget, abi.encodeWithSignature("totalLiquidity()"));
        uint256 burnAmount = liquidity == 0 ? 1 : ((rng % (liquidity + 5)) + 1);
        traceLog = _appendTrace(traceLog, string.concat("burn:", vm.toString(burnAmount)));
        _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("burn(uint256)", burnAmount), 0, address(this), "amm_pair_subset", seed, step, traceLog);
        return traceLog;
    }

    function _randomDiffAmmSwap(
        address solTarget,
        address vyperTarget,
        uint256 seed,
        uint256 step,
        uint256 rng,
        string memory traceLog
    ) internal returns (string memory) {
        (uint256 reserve0Before,) = _readReserves(solTarget);
        uint256 amount0Out = reserve0Before == 0 ? 0 : rng % (reserve0Before + 1);
        uint256 amount0In = (rng % 13) + 1;
        traceLog = _appendTrace(traceLog, string.concat("swap:", vm.toString(amount0Out), ":0:", vm.toString(amount0In), ":1"));
        _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("swap(uint256,uint256,uint256,uint256)", amount0Out, 0, amount0In, 1), 0, address(this), "amm_pair_subset", seed, step, traceLog);
        return traceLog;
    }

    function _randomDiffAmmSync(
        address solTarget,
        address vyperTarget,
        uint256 seed,
        uint256 step,
        uint256 rng,
        string memory traceLog
    ) internal returns (string memory) {
        uint256 balance0 = (rng % 500) + 1;
        uint256 balance1 = ((rng / 19) % 500) + 1;
        traceLog = _appendTrace(traceLog, string.concat("sync:", vm.toString(balance0), ":", vm.toString(balance1)));
        _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("sync(uint256,uint256)", balance0, balance1), 0, address(this), "amm_pair_subset", seed, step, traceLog);
        return traceLog;
    }

    function _property_counter(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        uint256 model = 3;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 3;
            string memory trace;
            bool ok;
            if (op == 0) {
                trace = "increment";
                traceLog = _appendTrace(traceLog, trace);
                (ok,,) = _run(target, abi.encodeWithSignature("increment()"), 0, address(this));
                _requireCheck(ok, "property", "counter", seed, i, traceLog, "counter increment failed");
                model += 1;
            } else if (op == 1) {
                uint256 amount = rng % 17;
                trace = string.concat("add_value:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                (ok,,) = _run(target, abi.encodeWithSignature("add_value(uint256)", amount), 0, address(this));
                _requireCheck(ok, "property", "counter", seed, i, traceLog, "counter add_value failed");
                model += amount;
            } else {
                trace = "reset";
                traceLog = _appendTrace(traceLog, trace);
                (ok,,) = _run(target, abi.encodeWithSignature("reset()"), 0, address(this));
                _requireCheck(ok, "property", "counter", seed, i, traceLog, "counter reset failed");
                model = 0;
            }
            _requireCheck(_readUint(target, abi.encodeWithSignature("value()")) == model, "property", "counter", seed, i, traceLog, "counter model mismatch");
        }
    }

    function _property_erc20_minimal(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        uint256 supply = _readUint(target, abi.encodeWithSignature("totalSupply()"));
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 5;
            uint256 amount = ((rng % 20) + 1) * 1 ether;
            string memory trace;
            if (op == 0) {
                trace = string.concat("transfer_this_to_BOB:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("transfer(address,uint256)", BOB, amount), 0, address(this));
            } else if (op == 1) {
                trace = string.concat("transfer_BOB_to_CAROL:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("transfer(address,uint256)", CAROL, amount), 0, BOB);
            } else if (op == 2) {
                trace = string.concat("approve_this_to_BOB:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("approve(address,uint256)", BOB, amount), 0, address(this));
            } else if (op == 3) {
                trace = string.concat("transferFrom_this_to_CAROL_by_BOB:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("transferFrom(address,address,uint256)", address(this), CAROL, amount), 0, BOB);
            } else {
                trace = string.concat("approve_BOB_to_CAROL:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("approve(address,uint256)", CAROL, amount), 0, BOB);
            }
            traceLog = _appendTrace(traceLog, trace);
            uint256 thisBalance = _readUint(target, abi.encodeWithSignature("balanceOf(address)", address(this)));
            uint256 bobBalance = _readUint(target, abi.encodeWithSignature("balanceOf(address)", BOB));
            uint256 carolBalance = _readUint(target, abi.encodeWithSignature("balanceOf(address)", CAROL));
            _requireCheck(_readUint(target, abi.encodeWithSignature("totalSupply()")) == supply, "property", "erc20_minimal", seed, i, traceLog, "total supply changed");
            _requireCheck(thisBalance <= supply && bobBalance <= supply && carolBalance <= supply, "property", "erc20_minimal", seed, i, traceLog, "sampled balance exceeds supply");
            _requireCheck(thisBalance + bobBalance + carolBalance == supply, "property", "erc20_minimal", seed, i, traceLog, "sampled balances do not sum to supply");
        }
    }

    function _property_vault_deposit_withdraw(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            address actor = _actor(rng);
            uint256 amount = ((rng % 9) + 1) * (1 ether / 10);
            string memory trace;
            if (rng % 2 == 0) {
                trace = string.concat("deposit_", _actorName(actor), ":", vm.toString(amount));
                _run(target, abi.encodeWithSignature("deposit()"), amount, actor);
            } else {
                trace = string.concat("withdraw_", _actorName(actor), ":", vm.toString(amount));
                _run(target, abi.encodeWithSignature("withdraw(uint256)", amount), 0, actor);
            }
            traceLog = _appendTrace(traceLog, trace);
            uint256 sampledShares =
                _readUint(target, abi.encodeWithSignature("balanceOf(address)", address(this))) +
                _readUint(target, abi.encodeWithSignature("balanceOf(address)", BOB)) +
                _readUint(target, abi.encodeWithSignature("balanceOf(address)", CAROL));
            uint256 totalShares = _readUint(target, abi.encodeWithSignature("totalShares()"));
            _requireCheck(sampledShares == totalShares, "property", "vault_deposit_withdraw", seed, i, traceLog, "sampled shares do not match total shares");
            _requireCheck(_readUint(target, abi.encodeWithSignature("totalAssets()")) == totalShares, "property", "vault_deposit_withdraw", seed, i, traceLog, "assets do not match shares");
        }
    }

    function _property_ownable_pausable(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        OwnableModel memory model = OwnableModel(address(this), false, 0, "");
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            model = _propertyOwnableStep(target, seed, i, rng, model);
        }
    }

    function _propertyOwnableStep(
        address target,
        uint256 seed,
        uint256 step,
        uint256 rng,
        OwnableModel memory model
    ) internal returns (OwnableModel memory) {
        uint256 op = rng % 4;
        address actor = _actor(rng);
        bool ok;
        if (op == 0) {
            model.traceLog = _appendTrace(model.traceLog, string.concat("pause_", _actorName(actor)));
            (ok,,) = _run(target, abi.encodeWithSignature("pause()"), 0, actor);
            _requireOwnableProperty(ok == (actor == model.owner), seed, step, model.traceLog, "pause authorization mismatch");
            if (ok) model.paused = true;
        } else if (op == 1) {
            model.traceLog = _appendTrace(model.traceLog, string.concat("unpause_", _actorName(actor)));
            (ok,,) = _run(target, abi.encodeWithSignature("unpause()"), 0, actor);
            _requireOwnableProperty(ok == (actor == model.owner), seed, step, model.traceLog, "unpause authorization mismatch");
            if (ok) model.paused = false;
        } else if (op == 2) {
            model.traceLog = _appendTrace(model.traceLog, string.concat("guardedIncrement_", _actorName(actor)));
            (ok,,) = _run(target, abi.encodeWithSignature("guardedIncrement()"), 0, actor);
            _requireOwnableProperty(ok == !model.paused, seed, step, model.traceLog, "paused guard mismatch");
            if (ok) model.counter += 1;
        } else {
            address newOwner = _actor(rng / 7);
            model.traceLog = _appendTrace(model.traceLog, string.concat("transferOwnership_", _actorName(actor), "_to_", _actorName(newOwner)));
            (ok,,) = _run(target, abi.encodeWithSignature("transferOwnership(address)", newOwner), 0, actor);
            _requireOwnableProperty(ok == (actor == model.owner), seed, step, model.traceLog, "ownership authorization mismatch");
            if (ok) model.owner = newOwner;
        }
        _requireOwnableProperty(_readAddress(target, abi.encodeWithSignature("owner()")) == model.owner, seed, step, model.traceLog, "owner model mismatch");
        _requireOwnableProperty(_readBool(target, abi.encodeWithSignature("paused()")) == model.paused, seed, step, model.traceLog, "paused model mismatch");
        _requireOwnableProperty(_readUint(target, abi.encodeWithSignature("counter()")) == model.counter, seed, step, model.traceLog, "counter model mismatch");
        return model;
    }

    function _property_amm_pair_subset(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 4;
            string memory trace;
            if (op == 0) {
                uint256 amount0 = (rng % 200) + 1;
                uint256 amount1 = ((rng / 17) % 200) + 1;
                trace = string.concat("mint:", vm.toString(amount0), ":", vm.toString(amount1));
                _run(target, abi.encodeWithSignature("mint(uint256,uint256)", amount0, amount1), 0, address(this));
            } else if (op == 1) {
                uint256 liquidity = _readUint(target, abi.encodeWithSignature("totalLiquidity()"));
                uint256 burnAmount = liquidity == 0 ? 1 : ((rng % liquidity) + 1);
                trace = string.concat("burn:", vm.toString(burnAmount));
                _run(target, abi.encodeWithSignature("burn(uint256)", burnAmount), 0, address(this));
            } else if (op == 2) {
                (uint256 reserve0Before, uint256 reserve1Before) = _readReserves(target);
                uint256 amount0Out = reserve0Before == 0 ? 0 : rng % (reserve0Before + 1);
                uint256 amount1Out = reserve1Before == 0 ? 0 : (rng / 11) % (reserve1Before + 1);
                uint256 amount0In = (rng % 13) + 1;
                uint256 amount1In = ((rng / 13) % 17) + 1;
                trace = string.concat("swap:", vm.toString(amount0Out), ":", vm.toString(amount1Out), ":", vm.toString(amount0In), ":", vm.toString(amount1In));
                _run(target, abi.encodeWithSignature("swap(uint256,uint256,uint256,uint256)", amount0Out, amount1Out, amount0In, amount1In), 0, address(this));
            } else {
                uint256 balance0 = (rng % 500) + 1;
                uint256 balance1 = ((rng / 19) % 500) + 1;
                trace = string.concat("sync:", vm.toString(balance0), ":", vm.toString(balance1));
                _run(target, abi.encodeWithSignature("sync(uint256,uint256)", balance0, balance1), 0, address(this));
            }
            traceLog = _appendTrace(traceLog, trace);
            uint256 reserve0 = _readUint(target, abi.encodeWithSignature("reserve0()"));
            uint256 reserve1 = _readUint(target, abi.encodeWithSignature("reserve1()"));
            uint256 totalLiquidity = _readUint(target, abi.encodeWithSignature("totalLiquidity()"));
            (uint256 reserve0FromPair, uint256 reserve1FromPair) = _readReserves(target);
            _requireCheck(reserve0 == reserve0FromPair && reserve1 == reserve1FromPair, "property", "amm_pair_subset", seed, i, traceLog, "reserve getter mismatch");
            _requireCheck(totalLiquidity == 0 || reserve0 + reserve1 > 0, "property", "amm_pair_subset", seed, i, traceLog, "liquidity without reserves");
        }
    }
